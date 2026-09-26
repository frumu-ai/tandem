import type { TandemClient, PackInspectionResponse, JsonObject } from "../src/index.js";

// Compile the public client return type as an actual downstream consumer.
declare const inspection: Awaited<ReturnType<TandemClient["packs"]["inspect"]>>;
const blueprint: JsonObject | undefined = inspection.pack.solution?.blueprint;
const activationRequired: boolean | undefined = inspection.pack.solution?.activation_required;
const materialized: boolean | undefined = inspection.pack.solution?.runtime_materialized;
void [blueprint, activationRequired, materialized];

// Legacy, non-solution inspection responses must remain valid.
const legacy: PackInspectionResponse = {
  pack: { installed: { pack_id: "legacy", name: "legacy", version: "1.0.0" } },
};
void legacy;
