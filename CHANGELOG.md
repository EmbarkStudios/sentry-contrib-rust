<!-- markdownlint-disable blanks-around-headings blanks-around-lists no-duplicate-heading -->

# Changelog
All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

<!-- next-header -->
## [Unreleased] - ReleaseDate
## [0.8.1] - 2023-11-15
### Fixed
- Commit [6adfcc4] pinned `sentry-types` to <=0.31.6 to avoid a breaking change in >=0.31.7.

## [0.8.0] - 2023-05-23
### Changed
- [PR#24](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/24) updated the underlying breakpad C++ library to ~HEAD. Thanks [@MarijnS95](https://github.com/MarijnS95)!

## [0.7.0] - 2022-12-16
### Changed
- [PR#22](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/22) bumped `sentry-core` to `>=0.29` which will hopefully mean we don't need to bump version numbers for new releases until there is an actual breaking change.

## [0.6.0] - 2022-11-04
### Changed
- [PR#18](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/18) bumped `sentry-core` to `0.28`.

## [0.5.0] - 2022-06-29
### Changed
- [PR#15](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/15) bumped `sentry-core` to `0.27`.

## [0.4.0] - 2022-05-26
### Changed
- Nothing. Bumping semver minor version since [0.3.1] was not technically semver correct.

## [0.3.1] - 2022-05-20
### Changed
- [PR#14](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/14) updated `sentry-core` to 0.26.0. Thanks [@poliorcetics](https://github.com/poliorcetics)!

## [0.3.0] - 2022-03-03
### Changed
- [PR#13](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/13) updated `sentry-core` to 0.25.0. Thanks [@vthib](https://github.com/vthib)!

### Removed
- [PR#11](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/11) removed some unused dependencies. Thanks [@vthib](https://github.com/vthib)!

## [0.2.0] - 2022-01-21
### Changed
- [PR#10](https://github.com/EmbarkStudios/sentry-contrib-rust/pull/10) updated `sentry-core` to 0.24.1. Thanks [@MarijnS95](https://github.com/MarijnS95)!

## [0.1.0] - 2021-07-27
### Added
- Initial implementation

<!-- next-url -->
[Unreleased]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.8.1...HEAD
[0.8.1]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.8.0...0.8.1
[0.8.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.7.0...0.8.0
[0.7.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.6.0...0.7.0
[0.6.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.5.0...0.6.0
[0.5.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.4.0...0.5.0
[0.4.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.3.1...0.4.0
[0.3.1]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.3.0...0.3.1
[0.3.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.2.0...0.3.0
[0.2.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/compare/0.1.0...0.2.0
[0.1.0]: https://github.com/EmbarkStudios/sentry-contrib-rust/releases/tag/0.1.0
