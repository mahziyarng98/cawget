# Release notes template

Copy this structure into a draft release and fill it from the actual release diff and test results.
Remove instructional text and empty sections before publishing. Link issues or pull requests where available.

## Changes

Describe the user-visible additions and fixes since the previous tag. Distinguish released behavior from plans.

## Upgrade notes

Document data migrations, configuration changes, keyring requirements, and any action an existing user needs.
State whether rollback was tested and which versions were used; otherwise say it was not tested.

## Packages

List the exact published asset names and architectures. Link the checksum file. State whether the Flatpak,
Arch recipe, or other package is from this release or an older one. Include bundled-library and licence information.

## Verification

Record the commit, CI run, automated checks, and actual installation or manual-test results. For each manual
check, give the package version, distribution version, desktop/session, result, and evidence link when available.
Separate maintainer-verified results from user-reported results and untested combinations.

## Known limitations

List unresolved regressions, unavailable packages, and relevant issue links. Keep planned features out of the
list of shipped changes.
