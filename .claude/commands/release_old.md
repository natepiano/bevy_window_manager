# Release Command

Perform a release for bevy_window_manager.

## Versioning Strategy

This crate uses a **branch-first release model**:
- **main branch**: Always has `-dev` versions (e.g., `0.18.0-dev`, `0.19.0-dev`)
- **release branches**: Created BEFORE publishing, contain actual release versions
- **Publishing**: Always happens from release branches, never from main

This crate tracks Bevy's release cycle:
- **Major.Minor** matches Bevy version (e.g., `0.17.x` for Bevy 0.17)
- **Patch** versions are independent (bug fixes, features that don't require Bevy changes)

Each release gets both a tag and a release branch with 1:1 correspondence to crates.io:
- Tag: `v0.17.1`
- Branch: `release-0.17.1`

## Usage
- `/release patch` - Auto-detect latest published version and increment patch (e.g., `0.18.0` → `0.18.1`). Skips STEP 10 (main stays at current dev version).
- `/release X.Y.Z-rc.N` - Release as RC version (e.g., `0.17.0-rc.1`)
- `/release X.Y.Z` - Release as final version (e.g., `0.17.0`)

## IMPORTANT: Version Handling in Commands

**Throughout this release process**, when you see `${VERSION}` in bash commands, you must substitute the actual version number directly (e.g., "0.17.0") instead of using shell variables.

**Example:**
- Documentation shows: `git checkout -b release-${VERSION}`
- You should run: `git checkout -b release-0.17.0`

This applies to ALL bash commands in this process.

## Prerequisites

Before starting the release, verify:
1. You're on the `main` branch
2. Working directory is clean (no uncommitted changes)
3. You're up to date with remote
4. `gh` CLI is installed and authenticated

<ProgressBehavior>
**AT START**: Dynamically generate and display the full progress list (once only):

1. Scan this document for all `## STEP N:` headers
2. Extract step number and description from each header
3. Count total steps and display as:

```
═══════════════════════════════════════════════════════════════
                 RELEASE ${VERSION} - PROGRESS
═══════════════════════════════════════════════════════════════
[ ] STEP 0:  <description from "## STEP 0: ..." header>
[ ] STEP 1:  <description from "## STEP 1: ..." header>
... (continue for all steps found)
═══════════════════════════════════════════════════════════════
```

**BEFORE EACH STEP**: Output single progress line using the total step count:
```
**[N/total] Step description...**
```
</ProgressBehavior>

<ExecutionSteps>
    **EXECUTE THESE STEPS IN ORDER:**

    Display <ProgressBehavior/> full list, then proceed:

    **STEP 0:** Execute <ArgumentValidation/>
    **STEP 1:** Execute <PreReleaseChecks/>
    **STEP 2:** Execute <CreateReleaseBranch/>
    **STEP 3:** Execute <BumpDevToRelease/>
    **STEP 4:** Execute <ChangelogVerification/>
    **STEP 5:** Execute <UpdateVersionCompatibility/>
    **STEP 6:** Execute <PublishCrate/>
    **STEP 7:** Execute <PushReleaseBranch/>
    **STEP 8:** Execute <CreateGitHubRelease/>
    **STEP 9:** Execute <PostReleaseVerification/>
    **STEP 10:** Execute <PrepareNextReleaseCycle/> **(Skip if `${IS_PATCH_RELEASE}` is true)**
</ExecutionSteps>

<ArgumentValidation>
## STEP 0: Argument Validation

**If argument is `patch`:**
1. Query crates.io for the latest published version:
```bash
curl -s "https://crates.io/api/v1/crates/bevy_window_manager" | jq -r '.crate.max_version'
```
2. Parse the version and increment the patch number (e.g., `0.18.0` → `0.18.1`)
3. Set `${VERSION}` to the incremented patch version
4. Set `${IS_PATCH_RELEASE}` to `true` (this will skip STEP 10)

**Otherwise, validate the version format** (must match `X.Y.Z` or `X.Y.Z-rc.N`):
→ **Auto-check**: Continue if version is valid format, stop with clear error if invalid
→ Set `${IS_PATCH_RELEASE}` to `false`

**Verify version is not already published on crates.io:**
```bash
curl -s "https://crates.io/api/v1/crates/bevy_window_manager" | jq -r '.versions[].num'
```
→ **Auto-check**: If `${VERSION}` appears in the list, stop with error: "Version ${VERSION} is already published on crates.io. Use a different version number."
→ Continue if version is not yet published

**Confirm version:**
```bash
echo "Release version set to: ${VERSION}"
```
</ArgumentValidation>

<PreReleaseChecks>
## STEP 1: Pre-Release Validation (on main)

### Git Status Check

```bash
git rev-parse --abbrev-ref HEAD
```
→ **Auto-check**: Must be on `main` branch, stop if not

```bash
git status --porcelain
```
→ **Auto-check**: Continue if clean (empty output), stop if uncommitted changes

```bash
git fetch origin
```

### Quality Checks

```bash
cargo clippy --all-targets --all-features -- -D warnings
```
→ **Auto-check**: Continue if no warnings, stop to discuss if there are issues

```bash
cargo build
```
→ **Auto-check**: Continue if builds, stop if errors

```bash
cargo +nightly fmt
```
</PreReleaseChecks>

<CreateReleaseBranch>
## STEP 2: Create Release Branch

**CRITICAL**: Create the release branch BEFORE any version changes. This keeps main at `-dev` versions.

```bash
git checkout -b release-${VERSION}
```
→ **Auto-check**: Continue if branch created successfully

**Verify you're on the release branch:**
```bash
git rev-parse --abbrev-ref HEAD
```
→ **Auto-check**: Should show `release-${VERSION}`

**Note**: All subsequent steps happen on this release branch. Main remains untouched with `-dev` versions.
</CreateReleaseBranch>

<BumpDevToRelease>
## STEP 3: Bump Version from -dev to Release

**Check current version (should be -dev):**
```bash
grep "^version" Cargo.toml
```

→ **I will update Cargo.toml** from `-dev` to the release version:
- Change `version = "X.Y.Z-dev"` to `version = "${VERSION}"`

**Commit the version bump:**
```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: bump version to ${VERSION}"
```
→ **Auto-check**: Continue if commit succeeds
</BumpDevToRelease>

<ChangelogVerification>
## STEP 4: Verify and Finalize CHANGELOG

```bash
head -20 CHANGELOG.md
```

→ **Manual verification**: Verify CHANGELOG.md has an `[Unreleased]` section with entries
  - Type **continue** to proceed
  - Type **stop** to add missing entry

→ **I will update CHANGELOG.md:**
Change `## [Unreleased]` to `## [${VERSION}] - $(date +%Y-%m-%d)`

```bash
git add CHANGELOG.md
git commit -m "chore: finalize CHANGELOG for v${VERSION}"
```
→ **Auto-check**: Continue if commit succeeds
</ChangelogVerification>

<UpdateVersionCompatibility>
## STEP 5: Update Version Compatibility Table

**Skip this step if:**
- This is a patch release (X.Y.Z where only Z changed from the previous release)
- The README already has a row for this major.minor version

**Update README.md** if this is a new major.minor version (e.g., 0.17 → 0.18):

Add a new row to the Version Compatibility table:
```markdown
| ${MAJOR}.${MINOR}   | ${MAJOR}.${MINOR} |
```

The table should show the latest minor version for each Bevy release.

```bash
git add README.md
git commit -m "docs: update compatibility table for v${VERSION}"
```
→ **Auto-check**: Continue after updating (or skipping if not needed)
</UpdateVersionCompatibility>

<PublishCrate>
## STEP 6: Publish to crates.io

**Dry run first:**
```bash
cargo publish --dry-run
```
→ **Manual verification**: Review package contents
  - Type **continue** to publish
  - Type **stop** to fix issues

**Publish:**
```bash
cargo publish
```
→ **Auto-check**: Continue if publish succeeds, stop if fails
</PublishCrate>

<PushReleaseBranch>
## STEP 7: Push Release Branch and Tag

**Create tag on release branch:**
```bash
git tag "v${VERSION}"
```
→ **Auto-check**: Continue if tag created

**Push the release branch:**
```bash
git push -u origin release-${VERSION}
```
→ **Auto-check**: Continue if push succeeds, stop if fails

**Push the tag:**
```bash
git push origin "v${VERSION}"
```
→ **Auto-check**: Continue if push succeeds, stop if fails
</PushReleaseBranch>

<CreateGitHubRelease>
## STEP 8: Create GitHub Release

→ **I will gather CHANGELOG entry and create a release using GitHub CLI**

```bash
gh release create "v${VERSION}" \
  --repo natepiano/bevy_window_manager \
  --title "bevy_window_manager v${VERSION}" \
  --notes "Release notes from CHANGELOG"
```
→ **Auto-check**: Continue if release created successfully, stop if fails
</CreateGitHubRelease>

<PostReleaseVerification>
## STEP 9: Post-Release Verification

```bash
curl -s "https://crates.io/api/v1/crates/bevy_window_manager" | jq '.crate.max_version'
```
→ **Manual verification**: Shows version ${VERSION}
  - Type **continue** to proceed
  - Type **retry** to check version again
</PostReleaseVerification>

<PrepareNextReleaseCycle>
## STEP 10: Prepare for Next Release Cycle

**Skip this step if `${IS_PATCH_RELEASE}` is true** - main is already at the correct dev version.

**Return to main branch:**
```bash
git checkout main
```
→ **Auto-check**: Continue if checkout succeeds

**Determine next dev version:**
- If released `0.18.0` or `0.18.0-rc.N`, next dev is `0.18.0-dev` (until final release)
- If released final `0.18.0`, next dev is `0.19.0-dev`

→ **I will ask**: What should the next dev version be?

**Update version on main to next -dev version** (if needed):
- `Cargo.toml`: `version = "${NEXT_DEV_VERSION}"`

→ **I will add [Unreleased] section to CHANGELOG.md**

Add this after the version header:
```markdown
## [Unreleased]

```

```bash
git add CHANGELOG.md Cargo.toml Cargo.lock
git commit -m "chore: prepare for next release cycle (${NEXT_DEV_VERSION})"
git push origin main
```
→ **Auto-check**: Continue if commit and push succeed

**✅ Release complete!** Crate published from release branch. Main stays at dev version.
</PrepareNextReleaseCycle>

## Branch Workflow Summary

**Standard Release (e.g., `/release 0.18.0`):**
```
main (0.18.0-dev) ─────────────────────────────────────────→ (0.19.0-dev)
         │                                                        ↑
         └─→ release-0.18.0 (0.18.0) ─→ publish ─→ tag v0.18.0   │
                                                                  │
                                              bump main to next dev
```

**Patch Release (e.g., `/release patch` when main is 0.19.0-dev):**
```
main (0.19.0-dev) ────────────────────────────────────────→ (stays 0.19.0-dev)
         │
         └─→ release-0.18.1 (0.18.1) ─→ publish ─→ tag v0.18.1
```
Use `/release patch` when main has bug fixes that are compatible with the previous release line. Main stays at its current dev version.

**Key Points:**
- Main ALWAYS has `-dev` versions
- Release branches are created BEFORE any version changes
- Publishing happens exclusively from release branches
- After standard release, main bumps to next `-dev` version
- Patch releases (`/release patch`) skip bumping main - use when backporting fixes
- Each release branch can receive patches independently

## Rollback Instructions

If something goes wrong after pushing but before publishing:

```bash
# Delete local tag
git tag -d "v${VERSION}"

# Delete remote tag
git push origin ":refs/tags/v${VERSION}"

# Delete release branch
git branch -D release-${VERSION}
git push origin :release-${VERSION}

# Return to main (which is unchanged)
git checkout main
```

If already published to crates.io, you cannot unpublish. You'll need to release a new patch version.

## Common Issues

1. **"Version already exists"**: The version is already published on crates.io
2. **"Uncommitted changes"**: Run `git status` and commit or stash changes
3. **"Not on main branch"**: Switch to main with `git checkout main`
4. **Build failures**: Fix any compilation errors before releasing
