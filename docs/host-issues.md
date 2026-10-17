# Host (v2 platform) issues observed while running on it

Upstream v2 platform bugs we CANNOT fix (we run ON the host; they affect
v2 itself identically). Each entry: mechanism, our evidence, impact on
orchestration, mitigation. Report upstream when the user directs.

## H-1 (2026-10-17, user-identified): the second report of a re-woken
subagent never reaches the parent

**Symptom (user's phrasing, confirmed by us):** a child subagent with a
background task reports once at its "end" (the parent treats it as
complete); the background task's completion WAKES the child; the child
finishes again and reports a SECOND time — and this second report is
never delivered to the parent agent.

**Our evidence trail (this session):**
- SPEC-R6: completed with the E3 canary still running (its report #1
  arrived, marked the task complete); the canary finished, the wake
  scheduler revived the child; the child produced its close-out commit
  (fe80161) and final report — the parent received NO notification; the
  report was only recovered when the user prompted and the parent called
  task_result manually (the content WAS there — delivery, not storage,
  is what failed).
- Repeated pattern: several lanes' post-revival finals were silently
  missed the same way; earlier related lifecycle incoherence: queued
  messages delivered to ALREADY-completed sessions cause ghost restarts
  (ROADMAP 2026-10-15), and the 60-min location-TTL reaper silently
  cancels children parked on background shells (ROADMAP 2026-10-15).

**Mechanism (from the 2.0.24/2.0.25 host source studied earlier):**
child completion = the job reaching its terminal state ->
`subagent-completion.deliver()` sends the synthetic completion
notification to the parent (report #1). The background task's completion
wakes the child session (a NEW generation), which runs and terminates
again — but the parent-side JOB is already terminal; a completed->
completed transition is not a completion transition, so the notification
path does not re-fire, and the parent's task board already shows the
task settled. The child's new final message is STORED in its session
(task_result returns it) — what is lost is the PUSH to the parent.
Effectively: the wake machinery and the job-completion machinery have
inconsistent lifecycle models (session generations vs job terminal
states).

**Impact on orchestration:** any child that parks work in the background
and gets re-woken produces work the parent will not see unless it polls;
combined with H-0 (TTL reaper) this makes long verification lanes
fragile in two independent ways.

**Mitigations (parent-side discipline, already in force / added):**
1. Lanes must not background long runs (the ≤45-min chunking rule) —
   reduces re-wake exposure to the wake scheduler's own revivals.
2. After ANY child known to have had a background task completes, the
   parent re-queries task_result on its next wake (cheap; recovers the
   stored-but-undelivered report). Added to the standing orchestration
   checklist.
3. The health-check census (per-wake activity query over live children)
   catches re-woken children as "active again after completion" — treat
   that signal as "a second report may exist; go read it."

**Disposition:** recorded, not fixable by us (host code). Candidate for
an upstream report if the user directs.
