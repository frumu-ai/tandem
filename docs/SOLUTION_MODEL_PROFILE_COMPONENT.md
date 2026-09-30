# Signed model-profile component

A solution pack may include one selected `model_profile` component whose artifact
is a version 1 model-profile catalog. The pack verifier checks its exact signed
bytes, pinned SHA-256, catalog schema, and the match between catalog classes and
the component's `model_classes` slots. A selected worker class must appear in
that catalog. Configuration preview requires a host-approved binding for every
declared class, including fallback and escalation classes.

Installation stages the catalog as a durable digest receipt through the existing
solution journal. Replaying staging revalidates the pack and catalog, then
observes the same receipt. No provider account, model request or native worker is
created by this component. The existing text fixture remains valid without one;
the signed-profile integration test adds a catalog to a synthetic copy of that
fixture so its old hash and behavior do not change.

Runtime activation still needs a current-user source/model authority factory:
fresh provider capability, availability, processing region, retention and price
evidence must be joined to the reviewed binding before profile selection and
each physical provider send. The staged catalog receipt alone is not dispatch
permission or proof of customer readiness.
