# Release Command

Perform a release for bevy_window_manager.

## Versioning Strategy

This crate tracks Bevy's release cycle:
- **Major.Minor** matches Bevy version (e.g., `0.17.x` for Bevy 0.17)
- **Patch** versions are independent (bug fixes, features that don't require Bevy changes)

Each release gets both a tag and a release branch with 1:1 correspondence to crates.io:
- Tag: `v0.17.1`
- Branch: `release-0.17.1`

This follows Bevy's release model and ensures every published version can be easily referenced.

## Usage
- `/release_version X.Y.Z-rc.N` - Release as RC version (e.g., `0.17.0-rc.1`)
- `/release_version X.Y.Z` - Release as final version (e.g., `0.17.0`)

## IMPORTANT: Version Handling in Commands

**Throughout this release process**, when you see `${VERSION}` in bash commands, you must substitute the actual version number directly (e.g., "0.17.0") instead of using shell variables.

## Prerequisites

Before starting the release, verify:
1. You're on the `main` branch (or appropriate release branch for patch releases)
2. Working directory is clean (no uncommitted changes)
3. `gh` CLI is installed and authenticated

<ExecutionSteps>
    **EXECUTE THESE STEPS IN ORDER:**

    **STEP 0:** Execute <ArgumentValidation/>
    **STEP 1:** Execute <PreReleaseChecks/>
    **STEP 2:** Execute <ChangelogVerification/>
    **STEP 3:** Execute <BumpVersion/>
    **STEP 4:** Execute <PublishCrate/>
    **STEP 5:** Execute <PushToGit/>
    **STEP 6:** Execute <CreateReleaseBranch/>
    **STEP 7:** Execute <CreateGitHubRelease/>
    **STEP 8:** Execute <PostReleaseVerification/>
    **STEP 9:** Execute <PrepareNextReleaseCycle/>
</ExecutionSteps>

<ArgumentValidation>
## STEP 0: Argument Validation

**Validate the version format** (must match `X.Y.Z` or `X.Y.Z-rc.N`):
→ **Auto-check**: Continue if version is valid format, stop with clear error if invalid
</ArgumentValidation>

<PreReleaseChecks>
## STEP 1: Pre-Release Validation

### Git Status Check

```bash
git status
```
→ **Auto-check**: Continue if clean, stop if uncommitted changes

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

<ChangelogVerification>
## STEP 2: Verify CHANGELOG Entry

```bash
head -20 CHANGELOG.md
```

→ **Manual verification**: Verify CHANGELOG.md has an entry for this version or an `[Unreleased]` section
  - Type **continue** to proceed
  - Type **stop** to add missing entry
</ChangelogVerification>

<BumpVersion>
## STEP 3: Bump Version

→ **I will check and update version if needed**

```bash
grep "^version" Cargo.toml
```

**Check current version:**
- If Cargo.toml already has `version = "${VERSION}"`, skip the Cargo.toml update
- If version differs, update `version = "${VERSION}"`

**Update CHANGELOG.md:**
Change `## [Unreleased]` to `## [${VERSION}] - $(date +%Y-%m-%d)` if present

**Commit changes (if any):**
- Only add files that were actually modified
- If Cargo.toml was unchanged, only add CHANGELOG.md (if it changed)
- If no files changed, skip the commit entirely and continue to next step

```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: bump version to ${VERSION}"
```
→ **Auto-check**: Continue if commit succeeds OR if no changes needed (version already correct)
</BumpVersion>

<PublishCrate>
## STEP 4: Publish to crates.io

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

<PushToGit>
## STEP 5: Push to Git

**Create and push tag:**
```bash
git tag "v${VERSION}"
git push origin main
git push origin "v${VERSION}"
```
→ **Auto-check**: Continue if push succeeds, stop if fails
</PushToGit>

<CreateReleaseBranch>
## STEP 6: Create Release Branch

**Skip this step if:**
- This is an RC release (X.Y.Z-rc.N)

**Create a release branch** with 1:1 correspondence to the crates.io version:

```bash
git branch release-${VERSION} v${VERSION}
git push origin release-${VERSION}
```

Example for version 0.17.1:
```bash
git branch release-0.17.1 v0.17.1
git push origin release-0.17.1
```

→ **Auto-check**: Continue if branch created and pushed successfully
</CreateReleaseBranch>

<CreateGitHubRelease>
## STEP 7: Create GitHub Release

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
## STEP 8: Post-Release Verification

```bash
curl -s "https://crates.io/api/v1/crates/bevy_window_manager" | jq '.crate.max_version'
```
→ **Manual verification**: Shows version ${VERSION}
  - Type **continue** to proceed
  - Type **retry** to check version again
</PostReleaseVerification>

<PrepareNextReleaseCycle>
## STEP 9: Prepare for Next Release Cycle

→ **I will add [Unreleased] section to CHANGELOG.md**

Add this after the version header:
```markdown
## [Unreleased]

```

```bash
git add CHANGELOG.md
git commit -m "chore: prepare CHANGELOG for next release cycle"
git push origin main
```
→ **Auto-check**: Continue if commit and push succeed

**✅ Release complete!**
</PrepareNextReleaseCycle>

## Development Workflow

Development happens on `main`. Each release creates its own `release-X.Y.Z` branch with 1:1 correspondence to the published crates.io version.

To reference a specific release:
- Use the tag: `git checkout v0.17.1`
- Use the branch: `git checkout release-0.17.1`

## Rollback Instructions

If something goes wrong after pushing but before publishing:

```bash
# Delete local tag
git tag -d "v${VERSION}"

# Delete remote tag
git push origin ":refs/tags/v${VERSION}"

# Delete release branch (if created)
git branch -d "release-${VERSION}"
git push origin ":release-${VERSION}"

# Revert the version bump commit
git revert HEAD
git push origin main
```

## Common Issues

1. **"Version already exists"**: The version is already published on crates.io
2. **"Uncommitted changes"**: Run `git status` and commit or stash changes
3. **"Not on main branch"**: Switch to main with `git checkout main`
4. **Build failures**: Fix any compilation errors before releasing
