# inodetree — Fast Linux Disk Space Analyzer in Rust

## 1. Overview & Product Goal

`inodetree` is a fast, open-source, rootless-first Linux disk space analyzer inspired by **WizTree**, written in Rust and packaged natively for RPM-based distributions (`.rpm`) alongside standard Linux binaries.

Unlike terminal-only tools (`dust`, `dua-cli`) or slow single-threaded GUI tools (`baobab`, `filelight`), `inodetree` combines:
1. **Zero-allocation-per-node arena memory layout** capable of holding millions of files in minimal RAM.
2. **A two-tier scanning engine**:
   - **Standard VFS mode (unprivileged, works everywhere):** Multi-threaded directory traversal using Linux `getdents64` + `statx` (`AT_STATX_DONT_SYNC | AT_SYMLINK_NOFOLLOW`), staying within a single filesystem boundary (`st_dev`) and deduplicating hardlinks by `(dev, ino)`.
   - **Turbo block-device mode (optional privilege elevation via `pkexec`):** Direct sequential read of raw `ext4` inode tables and directory blocks (and `$MFT` on mounted NTFS partitions) to scan entire NVMe drives in ~1 second, just like WizTree does on Windows.
3. **A native, immediate-mode GUI (`eframe` / `egui`)** compiled into a single lightweight binary with no Electron/WebKit dependencies, featuring WizTree's signature three-pane layout:
   - Hierarchical directory tree sorted by size.
   - File extension breakdown panel with live filtering/highlighting.
   - Interactive **Squarified Treemap** color-coded by file type with drill-down navigation.
4. **First-class `.rpm` packaging** via `cargo-generate-rpm`, complete with `.desktop` entry and scalable SVG icon.

---

## 2. Technology Stack & Crates

- **Language:** Rust (Edition 2024, MSRV 1.85+)
- **GUI:** `eframe` + `egui` (with `wgpu` or `glow` backend; keep features minimal for fast builds and small binary size)
- **Parallelism & Concurrency:** `rayon` (work-stealing directory scan & bottom-up aggregation), `crossbeam-channel` (progress snapshots to UI)
- **Linux Syscalls:** `libc` (`statx`, `getdents64`, `pread64`), `nix` (optional, only if cleaner for `ioctl`s)
- **CLI & Diagnostics:** `clap` (derive) for `--headless` benchmark/JSON output and launching directly into a target path (`inodetree /var`)
- **Desktop Integration:** `trash` (Freedesktop Trash specification support), `open` (`xdg-open` for "Open in File Manager"), `arboard` (copy path to clipboard)
- **Packaging:** `cargo-generate-rpm` configured in `Cargo.toml`

---

## 3. Repository Structure

Keep the core engine (`arena`, `scan`, `stats`, `mounts`) strictly decoupled from `ui` so that all scanning, deduplication, aggregation, and treemap layout logic can be tested headlessly in CI without a display server.

```text
inodetree/
├── Cargo.toml
├── LICENSE                       # MIT
├── README.md
├── assets/
│   ├── inodetree.desktop
│   └── inodetree.svg
├── .github/
│   └── workflows/
│       └── ci.yml                # fmt, clippy, test, cargo-generate-rpm
├── src/
│   ├── main.rs                   # CLI parsing, headless mode vs eframe launch
│   ├── lib.rs                    # Public core modules for integration tests
│   ├── arena.rs                  # Flat Vec<Node> + contiguous StringPool
│   ├── mounts.rs                 # Parse /proc/self/mountinfo for drive selector
│   ├── stats.rs                  # Extension breakdown & Top-N largest files
│   ├── actions.rs                # Trash, permanent delete, open, copy path + arena mutation
│   ├── scan/
│   │   ├── mod.rs                # ScanConfig, ScanProgress, ScanResult, HardlinkSet
│   │   ├── vfs.rs                # Parallel getdents64 + statx walker
│   │   └── ext4.rs               # Milestone 5: raw block device superblock & inode scanner
│   └── ui/
│       ├── mod.rs
│       ├── app.rs                # eframe::App state machine, toolbar, keyboard shortcuts
│       ├── tree_view.rs          # Virtualized expandable directory tree table
│       ├── ext_view.rs           # Extension summary table & color legend
│       └── treemap.rs            # Squarified Treemap layout & egui::Painter renderer
└── tests/
    ├── vfs_scan.rs               # Integration tests: symlinks, hardlinks, one-fs, permissions
    └── arena_stats.rs            # Aggregation, deletion updates, treemap rect invariants
```

---

## 4. Core Data Structures (`src/arena.rs`)

Never allocate a `Box`, `Arc`, or individual `String` per file. Scanning 3,000,000 files with heap-allocated nodes wastes hundreds of megabytes and stalls drop/cleanup.

```rust
pub type NodeId = u32;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum NodeKind {
    Directory = 0,
    File = 1,
    Symlink = 2,
    Other = 3,
}

/// Kept compact (<= 48 bytes) so 1M nodes fit in ~48 MB of contiguous RAM.
#[derive(Debug, Clone)]
pub struct Node {
    pub parent: NodeId,           // u32::MAX for root
    pub first_child: NodeId,      // u32::MAX if none
    pub next_sibling: NodeId,     // u32::MAX if none
    pub name_offset: u32,         // Byte offset into StringPool
    pub name_len: u16,            // Byte length in StringPool
    pub ext_id: u16,              // Index into ExtensionTable (0 = none/dir)
    pub kind: NodeKind,
    pub is_deleted: bool,         // Tombstone for instant UI deletion without re-indexing
    pub logical_size: u64,        // stx_size (or sum of children for directories)
    pub allocated_size: u64,      // stx_blocks * 512 (actual disk usage, handles sparse files)
    pub file_count: u32,          // Recursive count of regular files/symlinks
    pub dir_count: u32,           // Recursive count of subdirectories
    pub mtime_secs: i64,          // Latest mtime in subtree
}
```

### Arena Invariants
1. **StringPool:** Append-only `Vec<u8>` storing UTF-8 (or lossy UTF-8) file names contiguously. `Node::name(&self, pool: &StringPool) -> &str` slices in $O(1)$ without allocation.
2. **Full Path Reconstruction:** Walk `parent` pointers up to root and join components on demand (only needed for visible rows, hover tooltips, and user actions).
3. **Post-scan Aggregation:** After parallel scanning finishes, sort each directory's children descending by `allocated_size` (or `logical_size`) and roll up `logical_size`, `allocated_size`, `file_count`, `dir_count`, and `mtime_secs` bottom-up.
4. **Instant Mutation on Delete:** When the user trashes/deletes a node in the UI, mark `is_deleted = true` recursively and subtract its `allocated_size`, `logical_size`, `file_count`, and `dir_count` up the ancestor chain in $O(\text{depth})$ time.

---

## 5. Scanning Engine (`src/scan/`)

### 5.1 VFS Scanner (`src/scan/vfs.rs`)
- **Syscalls:** Use `libc::open` (`O_RDONLY | O_DIRECTORY | O_NOFOLLOW | O_CLOEXEC`) + `libc::getdents64` with a 64 KiB buffer per worker thread, paired with `libc::statx` using `libc::AT_SYMLINK_NOFOLLOW | libc::AT_STATX_DONT_SYNC` and mask `STATX_TYPE | STATX_MODE | STATX_SIZE | STATX_BLOCKS | STATX_MTIME | STATX_INO`.
- **Single Filesystem Boundary:** Capture `root_dev` via `statx` on the scan root. Skip any directory whose `stx_dev` differs from `root_dev` (prevents crossing into `/proc`, `/sys`, `/dev`, `/run`, or remote FUSE/NFS mounts when scanning `/`).
- **Hardlink Deduplication:** Maintain a sharded concurrent set of `(stx_dev, stx_ino)` for entries with `stx_nlink > 1`. The first encounter records full `logical_size` and `allocated_size`; subsequent encounters create the tree node with `0` allocated/logical size (marked as a hardlink duplicate) so total disk space matches `df`.
- **Symlinks:** Never follow directory symlinks (`AT_SYMLINK_NOFOLLOW`). Record symlinks as leaf `NodeKind::Symlink` entries so cycles cannot happen.
- **Error Handling:** Permission errors (`EACCES`, `EPERM`) increment an `unreadable_dirs: AtomicU64` counter shown softly in the status bar without aborting the scan.
- **Cancellation:** Poll an `Arc<AtomicBool>` cancellation token so clicking "Stop" or switching drives aborts cleanly within milliseconds.

### 5.2 Mountpoint Discovery (`src/mounts.rs`)
- Parse `/proc/self/mountinfo` to list real local filesystems (`ext4`, `btrfs`, `xfs`, `ntfs`, `ntfs3`, `vfat`, `exfat`, `zfs`, `f2fs`) along with their block device path (e.g., `/dev/nvme0n1p2`), mountpoint (`/`, `/home`, `/mnt/windows`), and `statvfs` total/free space.
- Ignore pseudo-filesystems (`proc`, `sysfs`, `tmpfs`, `cgroup2`, `overlay`, `squashfs`, `devtmpfs`).

---

## 6. GUI Specification (`src/ui/`)

### 6.1 Layout (WizTree-inspired)
- **Top Toolbar:**
  - Drive / Mountpoint combo box showing `[Mountpoint] — [Device] — [Used / Total]`, plus a `"Select Folder..."` button (`rfd` or built-in path input) and `"Scan"` / `"Stop"` button.
  - Toggle between **Allocated Size** (`stx_blocks * 512`, default) and **Logical Size** (`stx_size`).
  - Search / filter box (`Ctrl+F`) supporting substring and glob/extension filter (e.g., `*.iso`, `node_modules`).
- **Main Split View (Top Half):**
  - **Left Pane (~68% width) — Directory Tree Table:** Columns: `Name` (with folder/file icon + color dot), `Size` (human-readable `GiB`/`MiB`/`KiB` + visual percentage bar), `% of Parent`, `Items`, `Files`, `Folders`, `Last Modified`. Virtualized rendering (`egui_extras::TableBuilder::body().rows(...)`) so expanding a folder with 200,000 files stays locked at 60 FPS.
  - **Right Pane (~32% width) — File Extensions Table:** Columns: `Extension`, `Color`, `Size`, `% of Scan`, `Files`. Clicking an extension highlights all matching files in the treemap; clicking again clears the filter.
- **Bottom Split View (Bottom Half) — Interactive Squarified Treemap (`src/ui/treemap.rs`):**
  - Implements the **Bruls-Huizing-van Wijk Squarified Treemap** algorithm over the currently zoomed directory node.
  - Culls rectangles smaller than `2x2` logical pixels so rendering millions of tiny files takes < 2 ms per frame.
  - Color palette mapped deterministically by `ext_id` (archives, video, audio, images, code, disk images, binaries, other).
  - **Interactions:**
    - Hover: floating tooltip with full path, allocated size, logical size, and modification date.
    - Single click: selects the node and auto-expands/scrolls to it in the Directory Tree Table.
    - Double click on a directory: zooms the treemap into that subtree and updates a clickable breadcrumb bar (`root > home > user > Videos`).
    - Right-click context menu (shared between Tree and Treemap): `Open in File Manager`, `Copy Full Path`, `Move to Trash`, `Delete Permanently` (with confirmation modal).
- **Bottom Status Bar:**
  - Elapsed scan time (e.g., `Scanned 1,482,910 files in 0.84s`), total allocated size, and warning badge if any directories returned `EACCES`.

---

## 7. `.rpm` Packaging & CI/CD

### 7.1 `Cargo.toml` Metadata
Configure `cargo-generate-rpm` directly in `Cargo.toml`:

```toml
[package.metadata.generate-rpm]
assets = [
    { source = "target/release/inodetree", dest = "/usr/bin/inodetree", mode = "0755" },
    { source = "assets/inodetree.desktop", dest = "/usr/share/applications/inodetree.desktop", mode = "0644" },
    { source = "assets/inodetree.svg", dest = "/usr/share/icons/hicolor/scalable/apps/inodetree.svg", mode = "0644" },
]
```

### 7.2 Release Profile
```toml
[profile.release]
strip = true
lto = "thin"
codegen-units = 1
```

### 7.3 GitHub Actions (`.github/workflows/ci.yml`)
- On push / PR: `cargo fmt --check`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.
- On git tag `v*`: build release binary, install `cargo-generate-rpm`, run `cargo generate-rpm`, and attach the `.rpm` (for `x86_64` and `aarch64`) + `.tar.gz` archive to the GitHub Release.

---

## 8. Engineering Rules

1. **Zero `unwrap()` in runtime paths:** Propagate errors cleanly or surface them in the UI status banner. `unwrap()` is permitted only inside `#[cfg(test)]` and `tests/`.
2. **No blocking on the UI thread:** Scanning, trash/delete operations, and mountpoint refreshes run on background threads; the `egui` update loop only reads progress counters or swaps in the completed `Arena`.
3. **Strict lints:** Enable `clippy::all` and `clippy::pedantic` (allowing noisy style lints where appropriate) and keep warnings at zero.

---

## 9. Phased Implementation Plan for Claude Code

Execute one milestone at a time, verifying `cargo test` and `cargo clippy --all-targets -- -D warnings` after each step:

- **Milestone 1 — Core Arena, Mounts & Parallel VFS Scanner:**
  Initialize the Cargo project, implement `src/arena.rs`, `src/mounts.rs`, `src/stats.rs`, and `src/scan/vfs.rs` (`getdents64` + `statx`, single-filesystem check, hardlink deduplication), add `--headless <PATH>` CLI mode printing scan stats and top directories/extensions, and write thorough unit + integration tests in `tests/`.
- **Milestone 2 — Native `egui` Shell, Tree Table & Extension Panel:**
  Implement `src/ui/app.rs`, `src/ui/tree_view.rs`, `src/ui/ext_view.rs`, and `src/actions.rs` (open, copy path, trash/delete with instant arena update). Connect background scanning with live progress indicator.
- **Milestone 3 — Squarified Treemap & Bidirectional Selection:**
  Implement `src/ui/treemap.rs` with squarified layout, pixel culling, extension color coding, hover tooltips, breadcrumb zoom, extension highlight filtering, and click-to-sync with `tree_view`.
- **Milestone 4 — `.rpm` Packaging, Desktop Assets & CI:**
  Add `assets/inodetree.desktop`, `assets/inodetree.svg`, `Cargo.toml` `[package.metadata.generate-rpm]`, `.github/workflows/ci.yml`, and `README.md` with Fedora/RHEL/openSUSE installation instructions.
- **Milestone 5 — Turbo `ext4` Raw Block Device Scanner:**
  Implement `src/scan/ext4.rs` to read the ext4 superblock, block group descriptors, and sequential inode/directory blocks directly from `/dev/...` (with `pkexec` helper fallback when unprivileged) for sub-second full-disk scans.