# Release Command

Perform a release for bevy_restore_windows.

## Usage
- `/release_version X.Y.Z-rc.N` - Release as RC version (e.g., `0.17.0-rc.1`)
- `/release_version X.Y.Z` - Release as final version (e.g., `0.17.0`)

## IMPORTANT: Version Handling in Commands

**Throughout this release process**, when you see `${VERSION}` in bash commands, you must substitute the actual version number directly (e.g., "0.17.0") instead of using shell variables.

## Prerequisites

Before starting the release, verify:
1. You're on the `main` branch
2. Working directory is clean (no uncommitted changes)
3. cargo-release is installed (`cargo install cargo-release`)

<ExecutionSteps>
    **EXECUTE THESE STEPS IN ORDER:**

    **STEP 0:** Execute <ArgumentValidation/>
    **STEP 1:** Execute <PreReleaseChecks/>
    **STEP 2:** Execute <ChangelogVerification/>
    **STEP 3:** Execute <BumpVersion/>
    **STEP 4:** Execute <PublishCrate/>
    **STEP 5:** Execute <PushToGit/>
    **STEP 6:** Execute <CreateGitHubRelease/>
    **STEP 7:** Execute <PostReleaseVerification/>
    **STEP 8:** Execute <PrepareNextReleaseCycle/>
</ExecutionSteps>

<ArgumentValidation>
## STEP 0: Argument Validation

**Validate the version format:**
```bash
bash .claude/scripts/release_version_validate.sh "$ARGUMENTS"
```
→ **Auto-check**: Continue if version is valid format, stop with clear error if invalid

**Confirm version:**
```bash
echo "Release version set to: $ARGUMENTS"
```
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

→ **I will update `Cargo.toml` version to ${VERSION}**

```bash
grep "^version" Cargo.toml
```

**Update Cargo.toml:**
Set `version = "${VERSION}"`

**Update CHANGELOG.md:**
Change `## [Unreleased]` to `## [${VERSION}] - $(date +%Y-%m-%d)` if present

**Commit the version bump:**
```bash
git add Cargo.toml Cargo.lock CHANGELOG.md
git commit -m "chore: bump version to ${VERSION}"
```
→ **Auto-check**: Continue if commit succeeds
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

<CreateGitHubRelease>
## STEP 6: Create GitHub Release

→ **I will gather CHANGELOG entry and create a release using GitHub CLI**

```bash
gh release create "v${VERSION}" \
  --repo natemccoy/bevy_restore_windows \
  --title "bevy_restore_windows v${VERSION}" \
  --notes "Release notes from CHANGELOG"
```
→ **Auto-check**: Continue if release created successfully, stop if fails
</CreateGitHubRelease>

<PostReleaseVerification>
## STEP 7: Post-Release Verification

```bash
curl -s "https://crates.io/api/v1/crates/bevy_restore_windows" | jq '.crate.max_version'
```
→ **Manual verification**: Shows version ${VERSION}
  - Type **continue** to proceed
  - Type **retry** to check version again
</PostReleaseVerification>

<PrepareNextReleaseCycle>
## STEP 8: Prepare for Next Release Cycle

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

## Rollback Instructions

If something goes wrong after pushing but before publishing:

```bash
# Delete local tag
git tag -d "v${VERSION}"

# Delete remote tag
git push origin ":refs/tags/v${VERSION}"

# Revert the version bump commit
git revert HEAD
git push origin main
```

## Common Issues

1. **"Version already exists"**: The version is already published on crates.io
2. **"Uncommitted changes"**: Run `git status` and commit or stash changes
3. **"Not on main branch"**: Switch to main with `git checkout main`
4. **Build failures**: Fix any compilation errors before releasing
