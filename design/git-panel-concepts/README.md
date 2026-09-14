# Selected Git workflow

The selected direction combines B's bottom-pinned commit panel with A's sidebar
history. The other directions, comparison board, and their exports were removed
at the user's request. Preserve compactness and Pideck's existing Flint style.
The images are editable Pencil references for the native implementation. Their
commit history, authors, timestamps, outgoing counts, and `notes.md` are synthetic
examples. The application uses real repository data and its existing file icons.

[Working changes: selected B](UizsX.png)

[Commit history: refined A](BBFE5.png)

## History refinement

- A connected commit timeline gives the history a clear reading order. The
  selected commit has a HEAD marker, a distinct timeline node, and the existing
  Flint selection fill. Rows remain 52 pixels tall.
- Date groups separate the timeline. `2 unpushed` identifies the outgoing
  group, and an `origin/master` boundary marks the start of pushed history.
  Text labels accompany the subtle green and neutral timeline segments.
- Subjects occupy the first line; author, time, and hash share the second.
  Changes, branch selection, and Push have distinct controls at the top.
  Load older at the bottom extends the history without replacing the selection.
- A compact commit header shows its message, description, publication state,
  author, full date, and copy-hash action.
- Changed files are visible directly above the diff instead of hidden in a
  picker. Each row shows its full path, added/deleted lines, and a small change
  distribution bar. The file group can collapse to recover diff space.
- Keep the existing unified/split and hunk navigation. The selected commit is
  compared to its parent, with the current file highlighted in the file list.

## Editable source

Created with Pencil MCP in the active document:

`C:/Users/devas/.pencil/documents/42722b85-a773-402c-9575-c5cdc403368d/pencil-new.pen`

Use Pencil MCP to read or edit the encrypted document. The two selected screens
are arranged side by side: working changes on the left and refined history on
the right. PNG names are the source node IDs: `UizsX` and `BBFE5`. Shared tree
component: `fLktM`. No alternative directions remain on the canvas.

## Preserved design

- Flint colors from `src/theme.rs`; DM Sans chrome and Instrument Serif wordmark
  follow `src/theme/terminal_manager.rs`.
- Sidebar stays 360 design pixels. File rows remain 28 logical pixels with
  16-pixel nesting, expansion chevrons, vertical guides, and folder counts.
- Keep the existing tree construction, hierarchy, selection, collapse state,
  scrolling, and keyboard navigation. Reuse the current `project_icon` assets in
  implementation; the concepts use library icon stand-ins.
- Remove the Git filter input, M/U file markers, and their footer legend.
  Preserve the separate Files panel's filtering behavior.
- Show added/deleted line counts for new text files too: `notes.md` demonstrates
  `+28 −0`. Do not invent numeric counts for binary or unreadable content; show
  concise descriptive metadata. Empty new text files show `+0 −0`.

## Interaction contract

- `+` stages the selected file. `−` unstages it. Section actions apply to that
  section. Keep line counts and the fixed action rail aligned; filenames
  truncate before these controls. Full path and action names remain accessible.
- Row actions are available on keyboard focus as well as pointer interaction.
  Tab reaches each action; Enter activates it. Existing tree arrow keys and
  Enter-to-open-diff behavior remain intact.
- The undo icon opens a scoped discard confirmation. The diff also offers Undo
  file and Undo hunk. Discarding an unstaged tracked change restores the index
  version, preserving already staged content. Treat discarding a new file as
  file removal and say so explicitly. Cancel preserves the file and selection.
- Commit uses staged content only. Require a message and staged changes; keep
  the message on failure. Ctrl+Enter commits staged content. Push is a separate
  labeled action in the selected B layout.
- Push works independently of Commit. Show the outgoing count and identify its
  destination in the accessible label/tooltip, e.g. `origin/master`. Offer
  Publish branch if there is no upstream; do not guess a remote when ambiguous.
- A push failure retains the local commit and offers Retry push, which must
  not create another commit. If the remote moved, offer Fetch and review.
- Disable conflicting repeated actions during an operation; preserve the last
  valid tree and draft. Surface concise actionable errors without raw Git
  output. Refresh status after an operation and ignore stale completions.
- History is read-only. The History button in B opens A's sidebar timeline.
  Select a commit, then select a visible changed-file row to review its
  parent-to-commit diff. The file list can collapse; larger lists use a bounded
  viewport. Selection is visible and keyboard accessible.
- The timeline example is a linear branch history, not an invented multibranch
  graph. HEAD identifies the branch tip. For merge commits, show their merge
  identity and allow choosing the comparison parent. Show an explicit base
  comparison for the first commit in a repository.
- Load older appends older commits, preserving selection and scroll position.
  It has loading, retry, and end-of-history states. Loading more history does
  not change the current file review.
- Up/Down select commits; Tab reaches row actions and the detail controls.
  Changes or Esc returns to the working tree, preserving its expansion, selection,
  scroll position, and commit draft. Preserve the last history selection too.
  Copy hash copies the full hash and briefly confirms `Copied`.

## Additional states for implementation

Keep the current tree while refreshing and show a small busy indicator. For a
clean worktree show `No working changes`; Push remains available if ahead. With
no commits show `No commits yet`. History loading and errors stay inside the
history surface with retry. Disable committing unresolved conflicts with a
short explanation. Allow long messages to scroll within the composer.

At narrow widths, truncate paths and compact secondary labels before shrinking
tree indentation or line counts. The tree viewport scrolls independently of B's
pinned composer. At increased UI scale, retain control sizes and expose overflow
actions by keyboard. Focus uses the existing Flint focus token. These layouts
need no decorative animation; any state transition respects reduced motion.
Long commit subjects truncate to one line in the compact list and remain
available in full in the detail view and accessible name. The description
expands for long commit bodies without forcing the sidebar rows taller.

## Validation and scope

Both selected screens were exported and visually reviewed. Pencil's resolved
layout checks, including component instances, reported no clipping or overlap.
The native implementation includes the selected commit panel, history timeline,
file review, staging, discard, commit, publish, push, and fetch operations. The
user explicitly requested discarding the previous uncommitted `diff.rs` edits;
those were restored before implementing the new workflow.

Final validation:

- `cargo fmt --all -- --check` passed.
- `cargo check --all-targets` passed.
- `cargo test --all-targets`: 133 passed, 0 failed, 2 existing tests ignored.
- `cargo build --bin pi-gui` produced `target/debug/pi-gui.exe`.
- `git diff --check` passed.

Tests cover literal-path staging, new-file line counts, unborn repositories,
stale discard and commit requests, preservation of staged content, individual
hunk discard, nested-project commit scope, root and merge commits, binary and
renamed files, pagination, and publish/push/fetch/rejection using a temporary
local bare remote. GPUI tests cover history rendering, the unchanged sidebar
width, file rows, read-only historical review, and commit-draft preservation.

The user tested the built application and reported it good on 2026-09-14.
Agent-driven Windows inspection was unavailable because Computer Use could not
connect to its native helper. The existing `proc-macro-error2` future-compatibility
warning remains; no dependencies were changed.
