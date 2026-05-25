# Incident Response Log - 2025-09-12

**Incident ID:** INC-2025-09-12-001
**Severity:** High
**Status:** Closed

## Timeline (UTC)

**09:14** - DevOps alerts channel #ops-alerts triggering "High CPU usage on DB-Primary".
**09:18** - On-call engineer (@sarah_ops) acknowledges alert. Logs into dashboard.
**09:25** - @sarah_ops identifies 500% spike in read queries from `service-analytics` account.
**09:30** - Database unresponsive. Primary replica failover initiated automatically.
**09:32** - Failover successful. Secondary replica taking traffic.
**09:45** - Incident Commander (@mike_sec) assumes control. War room created.
**10:15** - Root cause identified: A malformed recursive query in the new "Weekly Report" cron job released at 09:00.
**10:20** - Fix deployed: Cron job disabled.
**11:00** - Verified system stability.

## Impact Analysis

- **Downtime:** 2 minutes (during failover).
- **Data Loss:** None.
- **Customer Impact:** 500 error responses for approx 1500 users during window.

## Lessons Learned

- Why was this query not caught in code review?
- We need better query timeout limits on the analytics user.
- Alerting threshold was slightly delayed; should trigger on 80% CPU not 95%.
