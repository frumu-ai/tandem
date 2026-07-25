FROM node:24-trixie-slim@sha256:ae91dcc111a68c9d2d81ff2a17bda61be126426176fde6fe7d08ab13b7f50573

ARG TARGETARCH

ENV DEBIAN_FRONTEND=noninteractive \
  TANDEM_ENGINE_VERSION=0.7.1 \
  TANDEM_ENGINE_BINARY_SHA256=f07c21e94680d53d3dad96e81239a229d08573fa29305d85290ca339b7f02cae \
  HOME=/var/lib/tandem/engine \
  XDG_CACHE_HOME=/var/lib/tandem/engine/.cache \
  npm_config_update_notifier=false \
  npm_config_fund=false \
  npm_config_audit=false

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates curl git \
  && rm -rf /var/lib/apt/lists/*

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
