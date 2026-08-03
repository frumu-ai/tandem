FROM node:24-trixie-slim@sha256:ae91dcc111a68c9d2d81ff2a17bda61be126426176fde6fe7d08ab13b7f50573

ARG TARGETARCH
ARG TANDEM_ENGINE_INSTALL_SOURCE=release

ENV DEBIAN_FRONTEND=noninteractive \
  TANDEM_ENGINE_VERSION=0.7.2 \
  TANDEM_ENGINE_BINARY_SHA256=a079e00720261b1bd950f099e18a67129e0b5b2c347c5db4b7631056a787ec73 \
  HOME=/var/lib/tandem/engine \
  XDG_CACHE_HOME=/var/lib/tandem/engine/.cache \
  npm_config_update_notifier=false \
  npm_config_fund=false \
  npm_config_audit=false

RUN rm -f /etc/apt/sources.list.d/debian.sources \
  && printf '%s\n' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian/20260720T000000Z trixie main' \
    'deb [check-valid-until=no] http://snapshot.debian.org/archive/debian-security/20260720T000000Z trixie-security main' \
    > /etc/apt/sources.list \
  && apt-get -o Acquire::Check-Valid-Until=false update \
  && apt-get install -y --no-install-recommends \
    ca-certificates=20250419 \
    curl=8.14.1-2+deb13u4 \
  && rm -rf /var/lib/apt/lists/* /etc/apt/sources.list

RUN --mount=type=bind,source=packages/tandem-control-panel/docker/release-candidate,target=/candidate,readonly \
    case "${TANDEM_ENGINE_VERSION}" in \
      ""|latest|next|beta|alpha) echo "ENGINE_VERSION must be an exact published version" >&2; exit 1 ;; \
    esac \
  && case "${TANDEM_ENGINE_INSTALL_SOURCE}" in \
      release|candidate) ;; \
      *) echo "TANDEM_ENGINE_INSTALL_SOURCE must be release or candidate" >&2; exit 1 ;; \
    esac \
  && case "${TARGETARCH:-amd64}" in \
      amd64) ;; \
      *) echo "No verified Tandem engine release asset is available for ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
  && npm install -g npm@12.0.1 \
  && mkdir -p /usr/local/lib/node_modules/@frumu/tandem/bin/native \
  && if [ "${TANDEM_ENGINE_INSTALL_SOURCE}" = candidate ]; then \
      candidate_package="/candidate/frumu-tandem-${TANDEM_ENGINE_VERSION}.tgz"; \
      test -f "${candidate_package}"; \
      test -f /candidate/tandem-engine; \
      npm install -g --ignore-scripts "${candidate_package}"; \
      install -m 0555 /candidate/tandem-engine \
        /usr/local/lib/node_modules/@frumu/tandem/bin/native/tandem-engine; \
    else \
      npm install -g --ignore-scripts @frumu/tandem@"${TANDEM_ENGINE_VERSION}"; \
      curl --fail --silent --show-error --location --retry 3 \
        "https://github.com/frumu-ai/tandem/releases/download/v${TANDEM_ENGINE_VERSION}/tandem-engine-linux-x64.tar.gz" \
        | tar -xz -C /usr/local/lib/node_modules/@frumu/tandem/bin/native tandem-engine; \
    fi \
  && node -p \
    "require('/usr/local/lib/node_modules/@frumu/tandem/package.json').version" \
    | grep -Fx "${TANDEM_ENGINE_VERSION}" \
  && printf '%s  %s\n' "${TANDEM_ENGINE_BINARY_SHA256}" \
    /usr/local/lib/node_modules/@frumu/tandem/bin/native/tandem-engine \
    | sha256sum -c - \
  && /usr/local/lib/node_modules/@frumu/tandem/bin/native/tandem-engine --version \
    | grep -F "${TANDEM_ENGINE_VERSION}" \
  && npm cache clean --force \
  && rm -rf /usr/local/lib/node_modules/npm /usr/local/lib/node_modules/corepack \
  && rm -f /usr/local/bin/npm /usr/local/bin/npx /usr/local/bin/corepack /usr/local/bin/pnpm /usr/local/bin/pnpx

RUN mkdir -p /var/lib/tandem/engine \
  && chown -R node:node /var/lib/tandem/engine

COPY packages/tandem-control-panel/docker/engine-entrypoint.sh /usr/local/bin/engine-entrypoint.sh
RUN chmod 0555 /usr/local/bin/engine-entrypoint.sh

EXPOSE 39731

USER node

ENTRYPOINT ["/usr/local/bin/engine-entrypoint.sh"]
