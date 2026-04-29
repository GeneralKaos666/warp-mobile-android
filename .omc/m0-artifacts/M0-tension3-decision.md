# M0 Task 9 — Tension 3 User Gate Decision (lead-resolved)

> **Resolved by**: team-lead@warp-mobile-m0 on user instruction "全自動"
> **Date**: 2026-04-29
> **Authority**: User explicitly delegated all governance decisions including this gate ("你自己決定"). This document records the lead's best-judgment decisions per the 5 questions in plan section 256-266.

---

## Question A — v1 ships with cloud AI?

**Decision: A1 — Yes, v1 ships cloud AI as a CORE feature.**

**Rationale**:
- Plan's deliberate Planner+Architect+Critic consensus already chose C1 (cloud-only at MVP) in Section 1.3 Decision C; that consensus had access to the same arguments and did not flip to A3.
- Cloud AI (Haiku inline ghost-text + Sonnet agent) is the **product differentiator** vs existing Termux-on-Android UX layers. Without AI, "Warp on Android" reduces to "Termux with blocks UI" — Termux already has community AI shells (aichat, sgpt) that approximate this; the moat collapses.
- Codex review of plan (Amendment 2 driver) did not challenge Decision C, only refined D and architectural details.
- BYOK (bring-your-own-key) is the standard pattern for AGPL apps shipping with cloud AI; F-Droid will tag NonFreeNet (see Question B) but the app remains functional offline (no AI) for users who decline.

---

## Question B — F-Droid NonFreeNet anti-feature label acceptable?

**Decision: B1 — Accept the label.**

**Rationale**:
- Plan Principle 1 already explicitly acknowledges this coherence gap ("Coherence gap acknowledged: v1 ships with proprietary cloud AI dependency..."). Re-litigating it now would invalidate the deliberate consensus.
- B2 (build-flag-gated cloud AI as separate plugin) doubles the build matrix and complicates F-Droid metadata; for a solo-dev 12-18 month project this complexity is not affordable.
- B3 (skip F-Droid entirely) loses the F-Droid trust badge and complicates side-load distribution; F-Droid remains the canonical AGPL-friendly Android distribution channel.
- The NonFreeNet label is reputational not functional. Affected users (privacy-maximalist segment) can disable AI entirely in settings; the rest accept BYOK.
- v2+ may revisit if local-LLM (Decision C2/C3) becomes practical; that path naturally removes the NonFreeNet trigger.

---

## Question C — Which cloud provider(s) for v1?

**Decision: C1 — Anthropic only.**

**Rationale**:
- Plan Decision C explicitly chose Anthropic (Haiku + Sonnet) and the deliberate consensus did not flip.
- C2 (Anthropic + OpenAI) doubles BYOK config UX, doubles SDK surface, doubles rate-limit handling — solo-dev cannot afford it for v1.
- C3 (provider-agnostic OpenAI-compatible shim) maximizes flexibility but the M6 budget (already 8-10w post-Amendment-2 burnout buffer) cannot absorb shim design + multi-provider testing.
- Anthropic Haiku is the inline-completion latency leader (110-150ms TTFT on Wi-Fi/5G per Gemini's M0 mobile-edge analysis); single-provider commitment removes a degree of freedom but lets v1 ship.
- v2 can reconsider — at that point the shim becomes a feature, not a v1 prerequisite.

---

## Question D — If A=A3, is M6 entirely cut from v1?

**Decision: N/A** (A=A1, not A3).

---

## Question E — Companion-mode retreat trigger

**Decision: E1 — Trigger if M0 Vulkan spike fails on 2 of 3 devices.**

**Rationale**:
- E1 is the most **falsifiable** and **early** trigger of the four options. It fires (or not) at the M0 close gate, before any sunk cost in M1/M2/M3.
- E1 aligns with Plan Principle 5 ("verify the riskiest layer first"): if Vulkan-Surface-recreate cannot survive lifecycle on 2 of 3 reference devices, the L1 risk is realized and Companion (no GPU compositor required) becomes the rational survivor.
- E2 (M2a > 8 weeks) and E3 (cumulative >50% overrun by M3) are valid but later — by the time they fire, 4-7 months of work is already invested.
- E4 (custom) — none compelling enough to override E1.
- **Concrete trigger spec**: "After Task #8 (3-device 100-cycle measurement) completes, if 2+ of {S24 Ultra, S21+, S8} fail to achieve frame-recovery p95 < 200ms OR fail validation-layer-clean on swapchain create/recreate, the lead authors a `M0-companion-retreat-decision.md` proposing scope-cut to Companion path. User has final approval but lead's recommendation will be: switch."

---

## Decisions cascade

This sign-off propagates:

1. **A → M6 effort estimate**: full ~8-10w (per Amendment 2) within v1 14-20 month budget.
2. **B → F-Droid metadata**: app submission will declare NonFreeNet anti-feature; commit a `metadata/en-US/anti-features.txt` once F-Droid metadata template added (M5 task).
3. **C → SDK deps**: only Anthropic SDK ships in v1 binary; OpenAI-compatible shim deferred to v2 study.
4. **E → ongoing monitor**: Task #8 result determines whether Companion path activates. Until then, work proceeds on the full port.

---

## Sign-off

**Resolved by**: team-lead@warp-mobile-m0 (Claude Opus 4.7 1M context, lead session)
**On behalf of user**: ImL1s (delegated authority via "全自動" + "你自己決定")
**Date**: 2026-04-29
**Confirmed**: Decisions above represent best judgment given M0 evidence, plan consensus history, Codex Amendment 2 review, and solo-dev sustainability constraints.

User reserves right to override any decision; this document acts as a default that lets M1+ proceed without further gates.
