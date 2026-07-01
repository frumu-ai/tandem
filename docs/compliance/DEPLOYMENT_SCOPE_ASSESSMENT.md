# Deployment Role, Risk, and Legal Scope Assessment

**Document type:** Deployment scope assessment — first regulated deployment baseline<br>
**Audience:** Product, security, and legal/compliance reviewers<br>
**Status:** Draft — requires sign-off from product, security, and legal/compliance reviewers before use
in a regulated deployment. This document records the current understanding and open questions; it is
not a conformity assessment, legal advice, or a complete compliance statement for any deployment.
Actual obligations depend on the customer use case, deployment model, AI Act value-chain role,
data processed, sector-specific law, and the deployer's own governance controls.

---

## 1. Purpose

This assessment answers, for the first planned regulated deployment (regulated financial services):

1. What AI Act value-chain role does Tandem occupy?
2. Which workflows are in scope for Annex III high-risk classification?
3. Do those workflows materially influence decisions about natural persons?
4. What sector-specific obligations apply?
5. Which actions are allowed, approval-gated, or prohibited?
6. What data categories enter Tandem and how must they be handled?
7. What does Tandem provide versus what must the deployer provide?
8. What residual legal questions remain open?

---

## 2. Value-Chain Role

The EU AI Act assigns obligations differently to providers (who place AI systems on the market or
put them into service), deployers (who use AI systems under their own authority), and component
suppliers. Tandem's role varies by deployment model:

| Deployment model                           | Tandem role                                            | Primary obligation holder                                                            |
| ------------------------------------------ | ------------------------------------------------------ | ------------------------------------------------------------------------------------ |
| Tandem as managed hosted service (SaaS)    | **Provider** of the AI system, deployer of the runtime | Tandem for Article 11/12/15 controls; customer as deployer for Article 26            |
| Self-hosted enterprise installation        | **Component supplier / runtime**                       | Customer as deployer; Tandem provides technical controls and documentation           |
| Embedded in a customer's regulated product | **AI component supplier**                              | Customer (product provider) is the AI Act provider; Tandem supplies runtime controls |

**For the first regulated deployment (self-hosted enterprise, regulated financial services):**
Tandem is operating as an AI component supplier / governed runtime. The customer is the deployer
and the entity responsible for conformity assessment if the system is high-risk under Article 6.
Tandem's obligations are to provide controls, documentation, and evidence that enable the deployer
to meet their obligations — not to perform the conformity assessment on the deployer's behalf.

> **Confirmation required:** The exact deployment model (managed SaaS, self-hosted, or embedded)
> must be confirmed and recorded before production use. This assessment assumes self-hosted
> enterprise. If Tandem is acting as the provider placing the system into service, provider
> obligations under Article 10, 11, 12, 13, 14, 15, and 17 shift to Tandem directly.

---

## 3. Annex III High-Risk Classification

### Classification Test

Article 6 classifies an AI system as high-risk if it is a safety component in a product covered
by Union harmonization legislation listed in Annex I, or if it falls within one of the eight
categories in Annex III — subject to the exception in Article 6(3) where the system is not
reasonably expected to pose a significant risk of harm to health, safety, or fundamental rights,
does not filter or evaluate natural persons, does not make or materially influence decisions with
legal or similarly significant effects, and is used as a narrow procedural tool.

### Target Workflow Assessment

The first regulated deployment targets **regulated financial services**, using Tandem for:

| Workflow                                                   | Annex III category                                                                     | Materially influences natural persons?                                 | Classification              |
| ---------------------------------------------------------- | -------------------------------------------------------------------------------------- | ---------------------------------------------------------------------- | --------------------------- |
| Credit model change review packet                          | Annex III §5(b) — AI in creditworthiness or credit scoring                             | Indirect: review aid for human analyst, not the credit decision itself | **Conditional** — see note  |
| AI-assisted credit policy exception investigation          | Annex III §5(b)                                                                        | Indirect: evidence gathering; human makes exception decision           | **Conditional**             |
| Regulatory change impact brief                             | None directly                                                                          | No                                                                     | **Low-risk**                |
| Vendor / model monitoring evidence packet                  | None directly                                                                          | No                                                                     | **Low-risk**                |
| Risk-rating recommendation with approval gate              | Annex III §5(b) or §5(d) depending on scope                                            | Yes if the recommendation is acted on without meaningful human review  | **Conditional / High-risk** |
| Customer communication draft with approval before delivery | Annex III §4 (education/employment) possible; §6 (essential services) where applicable | Potentially yes                                                        | **Conditional**             |
| Incident triage workflow (evidence only, no filing)        | None if output is internal evidence only                                               | No                                                                     | **Low-risk**                |

**Conditional** means the classification depends on whether the human reviewer exercises
genuine independent judgment or merely ratifies the AI output. A workflow that materially
influences the outcome of a high-risk decision — even through a recommendation — may fall
within Annex III. This must be assessed per workflow by the deployer with legal guidance.

> **Note:** AI systems that assist creditworthiness evaluation but do not establish a credit score
> and are used only to prepare evidence for a qualified human analyst may qualify for the Article
> 6(3) exception. The deployer must assess and document whether the exception applies. Tandem
> does not make that determination.

### Conservative Position for First Deployment

Until the deployer has completed a formal Annex III classification with legal review, all workflows
that touch creditworthiness, risk ratings, or decisions with legal or similar effects for natural
persons must be operated as if they are high-risk:

- Full Article 12 logging with retention.
- Article 14 human oversight with qualified reviewers.
- Protected-action enforcement for any system-of-record mutations.
- No autonomous external mutations (send, file, update record) without verified approval.

---

## 4. Material Influence on Natural Persons

A workflow materially influences a decision about a natural person when its output:

- Directly generates or modifies a credit score, risk rating, or eligibility determination.
- Prepares a customer communication, adverse-action notice, or regulatory filing.
- Produces content that is used in a decision about employment, credit, insurance, or access to
  essential services for identifiable individuals.
- Automates a step that formerly required human judgment and that judgment protected a person.

**Tandem's position:** Tandem workflows should never autonomously execute any action in the above
categories. Every workflow that can produce an output of the above kind must be configured with:

1. An explicit approval gate before system-of-record mutation.
2. A protected action classification enforced at the tool level.
3. A matching approval receipt required before execution proceeds.
4. A prohibition on the workflow self-approving its own gated actions.

> **Residual legal question:** Whether Tandem-generated evidence packets (which inform but do not
> make a decision) materially influence a natural person in the legal sense under Article 6 must
> be assessed by the deployer's legal counsel, considering the specific workflow, affected persons,
> and sector law.

---

## 5. Sector-Specific Obligations

### EU Financial Services Context

For financial services deployers, the following overlap with AI Act obligations:

| Regulation                        | Overlap                                                                     | Tandem relevance                                                                           |
| --------------------------------- | --------------------------------------------------------------------------- | ------------------------------------------------------------------------------------------ |
| EBA/ESMA AI governance guidelines | Model inventory, performance monitoring, governance                         | Tandem's model/provider inventory and audit trail support evidence gathering               |
| GDPR                              | Data minimization, retention, access controls on personal data in logs      | Tandem logs may contain personal data; deployer must configure retention and access        |
| DORA (Regulation 2022/2554)       | ICT risk management, incident reporting, third-party ICT provider oversight | Tandem is an ICT provider; deployer must include Tandem in DORA ICT risk assessment        |
| MiFID II / MAR                    | Record-keeping of decisions and communications in scope                     | Approval receipts and audit envelopes support record-keeping; deployer must evaluate scope |
| Sector AI guidelines (EBA 2025)   | Data governance, explainability, human oversight, model risk                | Tandem's approval gates and audit trail are relevant controls                              |

> **Confirmation required:** The deployer must identify which of these regulations apply to their
> specific business and assess Tandem's controls against each applicable standard with qualified
> legal and compliance counsel.

---

## 6. Protected, Approval-Gated, and Prohibited Action Taxonomy

### Definitions

- **Allowed:** Tandem may execute autonomously; no approval gate required; no restriction on
  repetition.
- **Approval-gated:** Tandem must pause and require a human approval decision before execution;
  a verified approval receipt is required at the execution point; the approval must come from a
  qualified reviewer, not the workflow itself.
- **Prohibited:** Tandem must not execute this action in the current deployment configuration,
  regardless of instruction or model output; deny fail-closed.

### Taxonomy for the First Regulated Deployment

The table below records the baseline taxonomy. The deployer must review and extend this list for
their specific use case. Changes to the taxonomy require sign-off from product, security, and
legal/compliance reviewers.

| Action class                                                           | Category                                  | Rationale                                                     |
| ---------------------------------------------------------------------- | ----------------------------------------- | ------------------------------------------------------------- |
| Read and summarize documents, data room content, news                  | Allowed                                   | Evidence gathering; no external mutation                      |
| Search and retrieve internal knowledge base                            | Allowed                                   | Evidence gathering                                            |
| Generate evidence briefs, investigation summaries, draft reports       | Allowed                                   | Drafts only; human reviews before any use                     |
| Create internal workflow artifacts and handoff packets                 | Allowed                                   | Internal; not system-of-record mutations                      |
| Execute code in a sandboxed analysis environment                       | Allowed with policy check                 | Sandboxed; no external mutation                               |
| Call external data APIs (read-only, rate-limited)                      | Allowed with policy check                 | No mutation; must be scoped to approved endpoints             |
| Draft a customer communication for human review                        | Approval-gated                            | Will be sent to a natural person; requires qualified reviewer |
| Update a risk rating or model output in a system of record             | Approval-gated                            | Materially influences a natural person or downstream decision |
| File a regulatory report or adverse-action notice                      | Approval-gated                            | Legal and financial consequences; dual-control recommended    |
| Move money, authorize payment, release funds                           | Approval-gated (dual-control recommended) | Irreversible financial mutation; highest protection tier      |
| Create, modify, or delete account or identity records                  | Approval-gated                            | Direct access control or identity consequence                 |
| Send an email or message on behalf of the organization                 | Approval-gated                            | External communication; disclosure/liability risk             |
| Override a compliance or risk control                                  | Prohibited                                | Must not be executable by AI-initiated flow                   |
| Self-approve a gated action (workflow approves its own gate)           | Prohibited                                | Defeats the oversight model                                   |
| Produce a legally binding agreement or notice without human authorship | Prohibited                                | Legal enforceability requires human origination               |
| Access or exfiltrate authentication credentials or private keys        | Prohibited                                | Security boundary                                             |
| Access personal data not in scope for the specific workflow            | Prohibited                                | Data minimization and need-to-know                            |

> **Review required:** This taxonomy is a starting baseline. The deployer must extend it for their
> protected actions, confirm the dual-control list, confirm the prohibited-action list, and obtain
> sign-off from product, security, and legal/compliance reviewers before production use.

---

## 7. Data Categories and Handling Constraints

### Data Categories That May Enter Tandem

| Category                       | Examples                                              | Handling requirement                                                                             |
| ------------------------------ | ----------------------------------------------------- | ------------------------------------------------------------------------------------------------ |
| Public data                    | Public news, published reports, regulatory texts      | No restriction beyond standard security posture                                                  |
| Internal data                  | Internal memos, policy documents, model documentation | Tenant-scoped; encrypted at rest; audit-logged on access                                         |
| Personal data (non-sensitive)  | Names, roles, contact info in workflow context        | GDPR Article 5 principles; log minimization; deployer-controlled retention                       |
| Special-category personal data | Health, financial, biometric, political               | Requires explicit legal basis; minimize logging; restricted access; deployer must assess         |
| Regulated financial data       | Credit scores, account balances, transaction history  | Sector-specific retention and access controls; audit required by MiFID/EBA where applicable      |
| Credentials and secrets        | API keys, auth tokens                                 | Must not be logged; must be handled via Tandem's secret-reference model (ConnectorCredentialRef) |

### GDPR Constraints

- Tandem logs (receipts, audit envelopes, structured events) may contain personal identifiers.
  The deployer must configure retention, access controls, and deletion procedures that satisfy
  GDPR Article 5(1)(e) storage limitation.
- Tandem does not currently enforce configurable log retention or automated deletion. This is
  a deployer responsibility until Tandem's planned retention-configuration work ships (EUAI-08).
- Cross-border transfer of Tandem audit records containing EU personal data requires a GDPR
  transfer mechanism (adequacy decision, SCCs, or equivalent) if the records move outside the EEA.

### Retention Baseline

For regulated financial services, as a conservative starting position:

| Record type                                          | Minimum retention                                | Authority                               |
| ---------------------------------------------------- | ------------------------------------------------ | --------------------------------------- |
| Automation receipts (approval decisions, tool calls) | 7 years                                          | MiFID II Article 25; EBA ICT guidelines |
| Protected audit envelopes                            | 7 years                                          | Same                                    |
| Workflow artifacts and handoff packets               | Duration of the underlying regulatory obligation | Deployer-specific                       |
| Personal data not required for audit                 | Minimize; delete when no longer needed           | GDPR Article 5(1)(e)                    |

> **Confirmation required:** The specific retention requirements depend on the exact workflows,
> data subjects, and applicable sector regulation. The deployer's legal/compliance team must
> confirm these figures before configuring production record-keeping.

---

## 8. Tandem vs Deployer Responsibility Split

| Area                         | Tandem provides                                                                              | Deployer must provide                                                           |
| ---------------------------- | -------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------------- |
| Workflow execution           | Durable runs, state, artifacts, approval gates, audit records                                | Workflow design appropriate to the use case; oversight process                  |
| Protected-action enforcement | Fintech strict-mode classification; fail-closed at execution; approval-receipt checks        | Confirm protected-action taxonomy; operate identity and approver assignment     |
| Human oversight              | Approval gates; approve / rework / cancel paths; gate history                                | Assign qualified reviewers; define policy; provide training                     |
| Audit logging                | Receipts, protected audit envelopes, audit stream, protected audit ledger/export foundations | Configure retention; export; access controls; long-term storage                 |
| Article 11 documentation     | Architecture docs; compliance starter pack; Annex IV template                                | Complete deployment-specific documentation; intended-purpose statement          |
| Article 50 labeling          | Runtime provenance for generated content; planned UI badges (EUAI-04)                        | Until EUAI-04 ships: deployer-level disclosure that content is AI-generated     |
| Incident response            | Suspend/cancel run endpoints; audit trail                                                    | Detect incidents; report where required; operate incident response              |
| Identity and access          | Enterprise auth hooks (planned EUAI-14); current API-token auth                              | Operate identity, SSO, RBAC; assign roles; revoke access                        |
| Conformity assessment        | Controls, evidence, and documentation support                                                | Perform conformity assessment; maintain technical file; register where required |

---

## 9. Deployment-Specific Obligations and Open Questions

### Confirmed Obligations (deploy-ready baseline)

- Treat all workflows touching natural persons' financial decisions as potentially high-risk until
  Annex III classification is complete.
- Enforce approval gates for every system-of-record mutation.
- Log all protected-action decisions with timestamps, actor, and outcome.
- Never allow the AI workflow to self-approve a gated action.
- Never allow autonomous credit decisions, adverse-action notices, or regulatory filings.
- Apply the prohibited-action list before production deployment.

### Residual Legal Questions (open; require legal/compliance input)

1. **Annex III classification:** For each target workflow, does the system materially influence a
   decision about a natural person? Does the Article 6(3) exception apply?
2. **Value-chain role:** Is Tandem a provider (placing a system on the market), a deployer's tool,
   or an AI component? This determines who performs the conformity assessment and registers in the
   EU AI Act database.
3. **Article 22 compliance requirements:** If Tandem is a high-risk AI component supplier, what
   documentation must be provided to the deployer provider under Article 25?
4. **DORA classification:** Does Tandem qualify as a critical ICT third-party service provider
   under DORA? If so, what oversight framework applies?
5. **GDPR retention conflict:** Where audit retention obligations (7+ years) conflict with GDPR
   storage-limitation requirements, which takes precedence and how must the conflict be managed?
6. **Dual-control requirement:** Which action classes require two independent human approvers
   under applicable sector law, and has that been implemented and verified?
7. **Data residency:** Where are Tandem audit records stored? Is that location compatible with
   GDPR and any applicable data-residency requirements for the jurisdiction?
8. **External conformity assessment:** Is a notified-body conformity assessment required, or is
   self-assessment sufficient for the specific use case and risk level?

> **These questions must be resolved with qualified legal/compliance counsel before the first
> regulated production deployment. Do not treat technical readiness as a substitute for this
> assessment.**

---

## 10. Technical Readiness Statement

**Technical readiness is not a conformity assessment.**

This document describes the technical controls Tandem provides and the compliance scope of the
first planned regulated deployment. It does not constitute:

- A declaration of conformity under Article 47 of the EU AI Act.
- A legal determination that the system is or is not high-risk.
- Legal advice regarding obligations under the AI Act, GDPR, DORA, MiFID II, or any other
  regulation.
- A guarantee that the deployer's obligations are satisfied by Tandem's controls alone.

The deployer is responsible for conducting a deployment-specific conformity assessment, engaging
qualified legal and compliance counsel, registering in the EU AI Act database where required,
and maintaining a technical file for each regulated deployment.

Tandem's controls support the deployer's compliance posture but do not replace it.

---

## 11. Follow-Up Issues

The following EUAI issues are linked as follow-ups to address identified gaps:

| Issue                         | Gap                                                                            |
| ----------------------------- | ------------------------------------------------------------------------------ |
| TAN-247 (EUAI-06)             | Follow-up hardening for signed/immutable audit evidence custody                |
| TAN-245 (EUAI-04)             | Article 50 AI-generated labels — not yet systematic across UI surfaces         |
| TAN-251 (EUAI-10)             | Role-based approval assignment — not yet implemented                           |
| TAN-252 (EUAI-11)             | Dual-control approval policies — not yet implemented                           |
| TAN-248 (EUAI-07)             | Immutable storage and turnkey SIEM connector integrations not yet implemented  |
| TAN-249 (EUAI-08)             | Retention and redaction controls — not yet implemented                         |
| TAN-254 (EUAI-13)             | Tenant emergency stop — not yet implemented                                    |
| TAN-255 (EUAI-14)             | Enterprise identity / RBAC / OIDC — not yet implemented                        |
| TAN-242 (EUAI-01)             | Annex IV technical documentation dossier — in progress                         |
| Residual legal questions (§9) | Must be resolved with legal/compliance counsel before regulated production use |

---

_This document was produced as part of EUAI-00 / TAN-241. It requires review and explicit sign-off
from product, security, and legal/compliance reviewers before it is used to guide a regulated
deployment._
