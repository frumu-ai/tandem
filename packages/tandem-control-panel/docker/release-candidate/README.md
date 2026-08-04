# Engine release candidate staging

Security Assurance places the locally built `tandem-engine` binary and the
packed `@frumu/tandem` npm package in this directory before building the
engine image with `TANDEM_ENGINE_INSTALL_SOURCE=candidate`. Docker mounts the
directory read-only during that build, so candidate artifacts are tested
without being copied into an image layer or committed to the repository.
