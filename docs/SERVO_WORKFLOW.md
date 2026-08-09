# Working with a patched Servo

retsurf does not build the engine from crates.io. It builds it from our Servo
fork, where the fixes retsurf needs live as ordinary commits. This is how a fix
travels: authored in the fork, consumed here, then sent upstream. Companion to
`docs/SERVO_PATCH.md`, which describes the fixes themselves.

## The map

| Where | What it is |
|---|---|
| `~/Repos/servo`, remote `upstream` | `servo/servo`. `main` plus `release/v0.5`, the branch the published `0.5.x` crates are cut from (`release/v0.4` is the previous line). |
| `~/Repos/servo`, remote `origin` | our fork, `mxmgorin/servo`. `main` is kept equal to `upstream/main` — nothing of ours lives there. |
| `retsurf-main` | **what retsurf builds.** Our fixes on top of `upstream/main`, i.e. unreleased Servo. Pushed to the fork; `Cargo.toml` pins its `rev`, and each pinned rev carries a `retsurf-main-<n>` tag — the branch itself is rebased onto upstream, so its commits move and only the tags stay put. |
| `retsurf-0.5` | the fallback line: the same fixes off `upstream/release/v0.5`, the tree the published `0.5.x` crates are cut from. Append-only, never force-pushed. Switch back by pointing every `rev` at it. `retsurf-0.4` is its retired predecessor. |
| `retsurf`, `patches/` | the same commits exported as plain `.patch` files (8 KB). Read-only mirror, not applied at build time. |
| `fix/<slug>` | one branch per fix, off `upstream/main`. This is what becomes a pull request. |

Both branches carry `retsurf: make the surfman connection optional` and
`retsurf: advance the containing-block walk past boxless ancestors`. Neither commit
is upstream-ready — both are AI-assisted and their messages say so; the upstream
versions get rewritten by hand (see §3).

**Tracking `main` is a deliberate trade.** It buys whatever landed since the last
release cut at the cost of an unfiltered tree: no curation, no backports, and
whatever is half-finished at that revision. Nothing retsurf has been chasing is
fixed there — the IntersectionObserver hang is our patch either way, declarative
shadow roots are still dropped by `DOMParser`, and `<audio>`/`<video>` and
`decodeAudioData` are still unimplemented. The gap narrows right after a release:
`release/v0.5` was cut 2026-08-04, and `main` was 79 commits past it on 2026-08-09.
Reassess when `release/v0.6` is cut.

### Why the whole workspace, not one crate

`Cargo.toml` patches all four of our direct dependencies on `servo/servo` —
`servo`, `servo-base`, `servo-media`, `servo-media-dummy` — and that pulls all 54
workspace crates from the fork. It has to be all or nothing: crates in the Servo
repo depend on each other by path (`{ workspace = true }` resolving to
`path = "components/..."`), so sourcing one crate from git drags its siblings out
of the same checkout. Mixed with crates.io copies, cargo ends up with two distinct
`servo-script` packages and the types stop matching at the boundary. The two media
crates are on that list because Servo absorbed them from `servo/media` — true of
`main` and of `release/v0.5`, but not of the older `0.4` line, where they are still
published separately and their entries come back out (see §2).

The pin is a `rev`, not a `branch`, so a build is reproducible; adding a fix means
bumping that `rev` deliberately.

### What this costs

- Cargo keeps a bare clone in `~/.cargo/git/db/servo-<hash>` (~1.7 GB) and reuses
  it. With `Cargo.lock` committed and the `rev` pinned, a normal build touches the
  network zero times; bumping the `rev` fetches only new objects.
- A cold `CARGO_HOME` pays the full clone once. That is every CI job without a
  cargo cache — today only `build-android.yml` has one (`Swatinem/rust-cache`).
- Building the engine from a different source recompiles the 54 workspace crates
  but nothing below them: measured **4m42s** for `cargo build --release`, because
  mozjs, webrender and stylo still come from crates.io unchanged.
- A fresh clone of retsurf no longer builds offline.

To seed the cargo cache from a local checkout instead of downloading (useful on a
slow link — it turned a 65-minute fetch into seconds):

```sh
DB=~/.cargo/git/db/servo-617624d7c3d34ecf      # name is <repo>-<hash of URL>
rm -rf $DB ~/.cargo/git/checkouts/servo-617624d7c3d34ecf
git clone --bare ~/Repos/servo $DB             # local clone: hardlinks, instant
git --git-dir=$DB remote set-url origin https://github.com/mxmgorin/servo
```

## 1. I have a fix — get it into retsurf

Write it in the fork first, even when retsurf is the urgent part: that is the copy
that has to be hand-authored (see §3), and the copy every later rebase replays.

```sh
cd ~/Repos/servo
git fetch upstream main
git switch -c fix/<slug> upstream/main
# edit components/<crate>/... by hand
./mach fmt && ./mach test-tidy
git commit -s -am "<crate>: <imperative sentence>"     # -s is required (DCO)

git switch retsurf-main && git cherry-pick fix/<slug>
git switch retsurf-0.4  && git cherry-pick fix/<slug>   # keep the fallback current

# Tag every rev retsurf pins: `retsurf-main` is rebased onto upstream, so the
# commit a shipped build points at only stays reachable through its tag. Never
# force-push retsurf-0.4 either — the fallback has revs pinned on it too.
git tag -a retsurf-main-<n> retsurf-main -m "Revision pinned by retsurf <version>"
git push -f origin retsurf-main
git push origin retsurf-0.4 retsurf-main-<n>
git rev-parse retsurf-main                             # -> new rev
```

Then in retsurf: put that sha in every `[patch.crates-io]` entry in `Cargo.toml`,
refresh the readable mirror with
`git -C ~/Repos/servo format-patch upstream/main..retsurf-main -o patches`,
then `cargo build --release`, `cargo test`,
`cargo check --no-default-features` (the handheld config), a `docs/SERVO_PATCH.md`
section for the new fix, a `CHANGELOG.md` entry, and — if the fix is a hang or a
crash — a harness page under `tests/pages/`.

The `// retsurf patch:` marker comments stay in the integration branches: they
make our edits greppable in a Servo checkout. They must never reach upstream —
which they can't, because a `fix/<slug>` branch is written from scratch by hand
rather than cherry-picked from `retsurf-*`.

### Iterating on the engine without a push

Point the patches at the checkout on disk; Servo edits then take effect on the
next `cargo build`, with no fetch and no commit:

```toml
[patch.crates-io]
servo = { path = "/home/mxmgo/Repos/servo/components/servo" }
servo-base = { path = "/home/mxmgo/Repos/servo/components/shared/base" }
```

Don't commit that — it is machine-specific and unpinned.

## 2. Following upstream, and going back to the release line

This is the default: retsurf pins a rev of `retsurf-main`. To move it forward,

```sh
cd ~/Repos/servo
git fetch upstream main
git rebase upstream/main retsurf-main       # our fixes stay on top
git push -f origin retsurf-main
git rev-parse retsurf-main
```

then update every `rev`, the tag, and `patches/` as in §1. To fall back to the
release line instead, point the `rev`s at `retsurf-0.5`. What to expect either way:

- **Dependency pins can collide.** `main` and `release/v0.5` both use the released
  RustCrypto stack, so neither needs a workaround. The retired `0.4` tree did: it
  pins `p256/p384/p521 =0.14.0-rc.14`, and the released `primeorder 0.14.0` adds a
  `WnafSize` bound the rc curves don't implement, so building against `retsurf-0.4`
  needs `primeorder = "=0.14.0-rc.14"` in `Cargo.toml` or resolution fails outright.
- A `[patch]` only applies if the source's version still satisfies our
  requirement. `main`'s workspace version is `0.5.0` since `release/v0.5` was cut
  (2026-08); when upstream bumps it again, bump `servo = "0.5"` too or the patch
  is silently unused — and `cargo` only *warns* about that, so lean on
  `tests/engine_source.rs` to catch it.
- **The workspace absorbs crates over time.** On `main` and on `release/v0.5`,
  `servo-media` and `servo-media-dummy` live in the Servo repo rather than on
  crates.io, so both are patched to the fork alongside `servo` — our WebAudio
  backend implements their traits, and a crates.io copy would put two of each crate
  in the graph and register the backend into the wrong one. Only the older `0.4`
  line needs those two `[patch]` entries removed. `stylo` is a git dependency
  (`servo/stylo`) on `main` and a crates.io crate on the release line; nothing of
  ours touches it directly.
- Expect API drift to break our code, though it stays small: measured 2026-07-28,
  `main` at `c2cb3d0e8b1` (440 commits past the `0.4` release point) built with
  **zero** changes to retsurf; measured 2026-08-09, `main` at `95333b6101a`
  (146 commits later) needed **one** import rename, `servo_media::audio::node` ->
  `audio_node`. Both engine fixes still held. Do each move on a branch anyway, and
  keep the fallback line current.

## 3. Sending the fixes upstream

```sh
cd ~/Repos/servo
git fetch upstream main
git rebase upstream/main fix/<slug>                 # must merge cleanly
./mach fmt && ./mach test-tidy
./mach test-wpt tests/wpt/tests/<area>/<test>.html
git push origin fix/<slug>                          # push output prints the PR link
```

Servo's own requirements, from the book's pull-request checklist:

- Claim the issue first — comment `@servo-highfive assign me` on it.
- Branch from `main`; rebase onto `main` if it stops merging cleanly.
- Every commit must compile and pass tests on its own, and carry a DCO sign-off
  (`git commit -s`).
- Add a test with the fix. For DOM/layout behaviour that means a web platform
  test; hangs count — a test that merely finishes is enough to catch one.
- Commits are squashed on merge and **the PR title and description become the
  final commit message**. Title: lower-case crate prefix, imperative sentence,
  no generic verbs ("layout: Advance past boxless ancestors ...", not "fix ...").
- There is no PR template. A workflow (`pull-request-wpt-export`) will mirror any
  `tests/wpt/tests/` change into a web-platform-tests PR on its own.

### AI policy — the hard constraint

Servo prohibits contributions generated by LLMs: code, documentation, pull
requests, issues, and comments. Allowed: understanding the codebase, and bug
finding **with manual verification**.

So the flow is one-directional: hand-written on `fix/<slug>` → cherry-picked into
`retsurf-*` → pinned here by `rev`. Never the reverse, or AI-assisted text reaches
a PR. Analysis notes (in the workshop repo under `docs/retsurf/`) are for you to
verify and rewrite, not to paste.

### After it lands

A landed fix should stop being ours to carry:

```sh
# which release branch is a published crate cut from?
gh api repos/servo/servo/commits/<sha from .cargo_vcs_info.json>/branches-where-head
```

On the next Servo release, create `retsurf-<new version>` off the new
`release/vX.Y` and cherry-pick only the fixes that are still unlanded — landed
ones drop out by themselves. Delete the corresponding section in
`docs/SERVO_PATCH.md`. When nothing is left to carry, delete `[patch.crates-io]`
entirely and go back to plain crates.io versions: that is the exit condition, and
without checking for it the fork pin becomes permanent.
