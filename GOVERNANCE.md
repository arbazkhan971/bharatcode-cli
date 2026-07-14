# BharatCode governance

BharatCode uses a lightweight maintainer model focused on transparent technical decisions and
reviewable changes.

## Roles

- Contributors report issues, propose designs, review changes, and submit pull requests.
- Maintainers review and merge changes in their areas, keep CI and releases healthy, and enforce
  the project's security and compatibility expectations.
- Core maintainers resolve cross-cutting architecture and release decisions. Current maintainers
  and ownership areas are listed in [MAINTAINERS.md](MAINTAINERS.md).

Roles are earned through sustained, constructive contribution. A maintainer may nominate a
contributor; existing maintainers approve role changes by consensus and record them in the
maintainers file.

## Decisions

Routine decisions happen through issues and pull requests. Material changes to security defaults,
provider contracts, persisted data, public APIs, licensing, or governance should begin with a
written proposal that explains alternatives, compatibility, migration, and verification.

Maintainers seek consensus. If consensus is not possible, core maintainers document a decision and
its rationale in the relevant issue or pull request. Conflicts of interest must be disclosed, and a
conflicted maintainer should not be the sole approver.

## Conduct and security

Participation is governed by [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md). Security vulnerabilities
should be reported privately as described in [SECURITY.md](SECURITY.md), not opened as public
issues before coordinated disclosure.

Changes to this document follow the same pull-request review process as other material project
policy changes.
