# Customer Support Ticket Digest - Q4 2025

**To:** Documentation Team
**From:** CX Lead
**Subject:** Recurring issues with API integration docs

Here are the top tickets from the last month related to our developer docs. Please cross-reference with our current guides. It seems like some of our verified specs might be out of date given the user confusion.

---

## Ticket #88291 - "API Rate Limits???"

**User:** dev_dave_99
**Priority:** High
**Message:**

> "I'm following your 'Getting Started' guide which says the free tier allows 1000 requests/minute. I'm getting 429 errors after just 100 requests! My app is crashing in prod because of this. Is the documentation wrong or is your API broken?"

**CX Note:** We might have changed these limits in the v2.1 update but didn't update the static docs?

---

## Ticket #88342 - "Python SDK Deprecation Warning"

**User:** sarah_j
**Priority:** Medium
**Message:**

> "The code snippet in 'Authentication Flow' uses the `auth.connect()` method. When I run this, my console screams that this method is deprecated and will be removed in 2026. It suggests `auth.establish_session()`. Why are you teaching us to use dead code?"

**CX Note:** Need to verify if `connect()` is indeed deprecated and update code blocks.

---

## Ticket #88501 - "Regional Endpoints failing"

**User:** enterprise_corp_IT
**Priority:** Critical
**Message:**

> "Your documentation lists `eu-west-1.api.platform.com` as the endpoint for European data residency. Pinging this returns a 404. We need GDPR compliance immediately."

**CX Note:** I heard DevOps might have consolidated EU regions into `eu-central`? Please check.

---

## Ticket #88992 - "SSO Configuration Confusion"

**User:** admin23
**Priority:** Low
**Message:**

> "The screenshot in the SAML setup guide shows a 'Save Configuration' button at the bottom right. On my dashboard, the button is on the top right and says 'Commit Changes'. I spent 10 minutes looking for the save button. Not a big deal but annoying."

**CX Note:** UI update wasn't reflected in screenshots.
