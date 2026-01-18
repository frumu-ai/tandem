# Alternative Release Workflow with Changelog Extraction

If you want to use CHANGELOG.md instead of auto-generated notes:

```yaml
- name: Extract release notes
  id: extract-notes
  run: |
    NOTES=$(node scripts/extract-release-notes.js ${{ github.ref_name }})
    echo "RELEASE_NOTES<<EOF" >> $GITHUB_OUTPUT
    echo "$NOTES" >> $GITHUB_OUTPUT
    echo "EOF" >> $GITHUB_OUTPUT

- name: Create Release
  id: create-release
  uses: actions/github-script@v7
  with:
    script: |
      const { data } = await github.rest.repos.createRelease({
        owner: context.repo.owner,
        repo: context.repo.repo,
        tag_name: context.ref.replace('refs/tags/', ''),
        name: `Tandem ${context.ref.replace('refs/tags/', '')}`,
        body: process.env.RELEASE_NOTES || 'See the assets below to download.',
        draft: true,
        prerelease: false
      });
      return data.id;
  env:
    RELEASE_NOTES: ${{ steps.extract-notes.outputs.RELEASE_NOTES }}
```

## Recommended Workflow

**Option 1 (Current - Simplest)**: Auto-generated notes

- Automatically creates notes from commits
- Configured via `.github/release.yml`
- No manual maintenance needed

**Option 2 (Manual CHANGELOG.md)**:

- More control over messaging
- Better for major releases
- Requires updating CHANGELOG.md before each release
- Use the script above to extract notes

## Best Practice

Use **both**:

1. Maintain CHANGELOG.md for major versions
2. Let GitHub auto-generate for patch releases
3. Edit release notes manually after publishing if needed
