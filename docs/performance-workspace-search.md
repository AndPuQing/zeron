# Workspace search

Workspace searches share a host-side index backed by
[`fff-search`](https://crates.io/crates/fff-search), pinned to `0.11.0`.

| Surface | RPC | Behavior |
| --- | --- | --- |
| Composer `@` | `SearchFiles` | Fuzzy file and folder names |
| File tree and command palette Files | `SearchWorkspaceFiles` | Fuzzy names; include-ignored mode uses a direct walk |
| Command palette In Files | `SearchWorkspaceContent` | Literal content search with smart case |

`WarmWorkspaceSearch` starts an index before the first query. Requests route to
the device that owns the workspace. Older hosts keep the existing name-search
protocol; the palette explains when content search requires a host update.

## Index lifecycle

`WorkspaceSearch`, owned by `Repos`, keys indexes by canonical workspace root.
The first warm-up or search starts a background scan and file watcher. The UI
warms the focused chat's index when focus changes.

- Content searches wait up to 300 ms for the initial scan and return
  `indexing: true` while results may be incomplete. Name searches without an
  indexing field wait up to two seconds.
- At most seven workspace indexes remain live. Creating another evicts the least
  recently used one. Idle indexes expire after 15 minutes; a reaper checks
  once per minute.
- Each index caches at most 16 MB of file contents. Indexes store paths and
  search content on demand, without a persistent frecency database or a
  content bigram index.
- Closing the new-project picker releases its home-directory index unless
  a chat also warmed that root.
- Eviction stops the scan and watcher. Linux also requests allocator memory
  reclamation; process RSS may retain allocator arenas.

## Matching and limits

Name matching tolerates typos and considers filename, Git recency and
modification time. An empty composer `@` query lists featured paths followed by
recently changed files.

Ignored files and symbolic links are excluded from indexed searches. Dotfiles
are indexed within Git repositories, but skipped in plain folders. `.git` is
always excluded. The file tree's include-ignored mode walks directly and ranks
case-insensitive substrings by name prefix, name and path.

Content queries use literal text and smart case, preserve spaces and
backslashes, and skip binary files and files larger than 10 MB. Results include
the file, line, byte column and UTF-8 byte ranges within a bounded preview.
Queries have a two-second search budget and may return partial results.

## Home-folder searches

The host's home folder uses a separate names-only index. It reads directory
listings without opening file contents, and skips cloud folders, hidden
folders, machine state and dependency directories. Content search is unavailable
at the home root. Open a specific project folder to search its contents.

The home index holds at most 250,000 entries. After one minute, the next query
starts a background refresh while the existing index continues answering.

New chats without a project use individual folders under the host's data
directory (`~/.zerun/scratch` by default). Their searches stay within that folder.
Existing chats that use the home folder retain their working directory.

## Development

The implementation is in `crates/engine/src/workspace_search.rs`. The search
dependency adds native C builds, so engine builds require a C compiler on each
platform. Existing mobile clients continue to use `SearchFiles`.

Run the benchmark against a chosen checkout:

```sh
BENCH_ROOT=/path/to/checkout cargo run --release -p zeron-engine \
    --example workspace_search_bench
```

`BENCH_QUERIES=a,b,c` selects queries and `BENCH_RUNS` sets the number of timed
runs. RSS reporting is available on Linux. Index lifecycle messages use the
`info` log level and include the searched root.
