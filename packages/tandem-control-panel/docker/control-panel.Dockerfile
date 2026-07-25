FROM node:24-trixie-slim@sha256:ae91dcc111a68c9d2d81ff2a17bda61be126426176fde6fe7d08ab13b7f50573 AS build

ENV PNPM_HOME=/pnpm \
  PATH=/pnpm:$PATH

RUN corepack enable \
  && corepack prepare pnpm@11.17.0 --activate

WORKDIR /workspace

COPY packages/tandem-control-panel/package.json packages/tandem-control-panel/pnpm-lock.yaml packages/tandem-control-panel/pnpm-workspace.yaml ./packages/tandem-control-panel/
COPY packages/tandem-client-ts/package.json ./packages/tandem-client-ts/package.json

RUN pnpm -C packages/tandem-control-panel install --frozen-lockfile

COPY packages/tandem-client-ts ./packages/tandem-client-ts
COPY packages/tandem-theme-contract ./packages/tandem-theme-contract
COPY packages/tandem-control-panel ./packages/tandem-control-panel

RUN pnpm -C packages/tandem-client-ts build \
  && pnpm -C packages/tandem-control-panel build

FROM node:24-trixie-slim@sha256:ae91dcc111a68c9d2d81ff2a17bda61be126426176fde6fe7d08ab13b7f50573

ENV DEBIAN_FRONTEND=noninteractive \
  HOME=/var/lib/tandem/panel \
  XDG_CACHE_HOME=/var/lib/tandem/panel/.cache \
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
  && rm -rf /var/lib/apt/lists/* /etc/apt/sources.list \
  && corepack enable \
  && corepack prepare pnpm@11.17.0 --activate

WORKDIR /opt/tandem

COPY packages/tandem-control-panel/package.json packages/tandem-control-panel/pnpm-lock.yaml packages/tandem-control-panel/pnpm-workspace.yaml ./packages/tandem-control-panel/
COPY packages/tandem-client-ts/package.json ./packages/tandem-client-ts/package.json
COPY --from=build /workspace/packages/tandem-client-ts/dist ./packages/tandem-client-ts/dist
COPY --from=build /workspace/packages/tandem-control-panel/bin ./packages/tandem-control-panel/bin
COPY --from=build /workspace/packages/tandem-control-panel/lib ./packages/tandem-control-panel/lib
COPY --from=build /workspace/packages/tandem-control-panel/server ./packages/tandem-control-panel/server
COPY --from=build /workspace/packages/tandem-control-panel/dist ./packages/tandem-control-panel/dist
COPY --from=build /workspace/packages/tandem-control-panel/src/generated ./packages/tandem-control-panel/src/generated
COPY --from=build /workspace/packages/tandem-control-panel/.env.example ./packages/tandem-control-panel/.env.example
COPY --from=build /workspace/packages/tandem-control-panel/README.md ./packages/tandem-control-panel/README.md

RUN pnpm -C packages/tandem-control-panel install --prod --frozen-lockfile \
  && pnpm store prune \
  && rm -rf /usr/local/lib/node_modules/npm /usr/local/lib/node_modules/corepack \
    /root/.cache/node/corepack /var/lib/tandem/panel/.cache /var/lib/tandem/panel/.local /pnpm \
  && rm -f /usr/local/bin/npm /usr/local/bin/npx /usr/local/bin/corepack /usr/local/bin/pnpm /usr/local/bin/pnpx \
  && mkdir -p /var/lib/tandem/panel/control-panel \
  && chown -R node:node /var/lib/tandem/panel /opt/tandem

COPY packages/tandem-control-panel/docker/control-panel-entrypoint.sh /usr/local/bin/control-panel-entrypoint.sh
RUN chmod 0555 /usr/local/bin/control-panel-entrypoint.sh

WORKDIR /opt/tandem/packages/tandem-control-panel

EXPOSE 39732

USER node

ENTRYPOINT ["/usr/local/bin/control-panel-entrypoint.sh"]
