# Maintenance and releases

This is the process for maintaining Cawget through successive releases. It records work and evidence;
it does not promise a release date or imply that a planned feature has shipped.

## Triage feedback

- Review new bug reports and installation feedback before preparing a release.
- Ask for the exact version, package, distribution, desktop session, and minimal reproduction when missing.
- Prioritize data loss, credential handling, download corruption, and regressions before new features.
- Link a confirmed report to its fix and regression test. Close it only with the version or commit containing
  the fix and the verification result. A successful local build is not an installation test.
- Keep compatibility claims tied to a specific release and environment. Mark user reports as user-reported
  until independently reproduced; never turn an unverified report into a tested-platform claim.

## Before tagging a release

1. Review changes since the last tag and the open issue and pull-request lists. Choose a bounded scope.
2. Run the checks in [CONTRIBUTING.md](../CONTRIBUTING.md) and confirm CI passes for the release commit.
3. Update the changelog with user-visible changes, linked fixes, upgrade notes, and remaining limitations.
4. Check the versions in Cargo manifests, the desktop package metadata, and packaging recipes. Update the
   README status and install examples for the packages that will actually be published.
5. Build packages using the release workflow. Verify their checksums and record which package/environment
   combinations were installed and launched. Keep untested combinations explicitly untested.
6. Check pause/resume, queue persistence after restart, and credential/keyring behavior on the environments
   available for testing. Do not claim a check was run if the environment was unavailable.
7. Prepare release notes using [the release template](RELEASE_TEMPLATE.md). Confirm links, filenames,
   checksums, third-party licence lists, and source-package information against the actual release assets.
8. Review the draft before publishing. A missing package must be called out; do not point users at a previous
   package as though it were built from the new release.

## After publishing

- Verify the published asset list and download links against the README and installation guide.
- Link resolved reports to the published release. Invite reporters to retest without claiming they have done so.
- Carry unresolved regressions and unsupported environments into the next release's notes.
- Collect actual installation reports through the installation feedback form; do not manufacture testimonials,
  download counts, or a history of maintenance.

## Current documentation boundary

At release 0.1.2, the published application packages are AppImage, .deb, and .rpm. The AppImage also has a
`.zsync` asset. The separately available Flatpak bundle is still 0.1.1. Existing manual-test dates and
limitations remain in [INSTALL.md](INSTALL.md); this guide does not extend that test coverage.
