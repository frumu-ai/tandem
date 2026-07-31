FROM node:24-trixie-slim@sha256:ae91dcc111a68c9d2d81ff2a17bda61be126426176fde6fe7d08ab13b7f50573

ARG TARGETARCH

ENV DEBIAN_FRONTEND=noninteractive \
  TANDEM_ENGINE_VERSION=0.7.2 \
  TANDEM_ENGINE_BINARY_SHA256=5286e28a15aaf5e3c61b785a45fb186697ac7ef40499e4fef2c0cbc23c852846 \
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

RUN case "${TANDEM_ENGINE_VERSION}" in \
      ""|latest|next|beta|alpha) echo "ENGINE_VERSION must be an exact published version" >&2; exit 1 ;; \
    esac \
  && case "${TARGETARCH:-amd64}" in \
      amd64) ;; \
      *) echo "No verified Tandem engine release asset is available for ${TARGETARCH}" >&2; exit 1 ;; \
    esac \
  && npm install -g npm@12.0.1 \
  && npm install -g --ignore-scripts @frumu/tandem@"${TANDEM_ENGINE_VERSION}" \
  && mkdir -p /usr/local/lib/node_modules/@frumu/tandem/bin/native \
  && curl --fail --silent --show-error --location --retry 3 \
    "https://github.com/frumu-ai/tandem/releases/download/v${TANDEM_ENGINE_VERSION}/tandem-engine-linux-x64.tar.gz" \
    | tar -xz -C /usr/local/lib/node_modules/@frumu/tandem/bin/native tandem-engine \
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
