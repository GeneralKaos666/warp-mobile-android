# M0 Task 9 — Tension 3 User Gate Decision

> **Goal**: lock the v1 cloud-AI scope before M1 starts. The choices made here cascade into the M6 effort estimate, the F-Droid anti-feature label, the AGPL §7 lawyer scope, and the Companion-mode retreat trigger.
>
> **How to use**: read each Question, write your answer below the `**Answer**:` line, replace `<…>` placeholders, sign at the bottom. There is no wrong answer; the goal is *commitment* so the rest of the plan can stop branching.

---

## Question A — v1 ships with cloud AI?

**Context**: Plan Decision C1 chose "cloud-only at MVP" (Anthropic Haiku for inline ghost-text, Sonnet for agent). C2 (local llama.cpp at MVP) was rejected because of OOM risk on 4GB-RAM mid-tier; C3 (hybrid from day 1) was rejected for doubling test matrix. **Amendment 1 doesn't change C1**, but the Tension 3 gate gives you a second chance to defer all AI to v2.

**Pick one**:

- [ ] **A1. Yes — v1 ships cloud AI as a CORE feature** (matches plan's C1 chosen option; M6 stays ~8-10 weeks within budget; F-Droid users must self-supply API key)
- [ ] **A2. Yes — v1 ships cloud AI as OPT-IN** (default-off; first-launch dialog asks; users who skip AI get a "plain Warp on Android" experience; reduces F-Droid friction)
- [ ] **A3. No — defer all AI to v2** (M6 cut entirely; v1 = blocks UI + Termux runtime + manual command entry only; saves ~8-10 weeks; v1 distinguishes from Termux only via blocks UX, not AI)

**Answer**: <pick A1 / A2 / A3>

**Rationale (1-2 lines)**: <explain why>

---

## Question B — F-Droid NonFreeNet anti-feature label acceptable?

**Context**: F-Droid will tag any app that requires/encourages a proprietary network service with the **NonFreeNet** anti-feature label. Anthropic API qualifies. The label is reputation-affecting in the F-Droid community but does not block listing. Plan Principle 1 already declared a coherence gap.

**Pick one** (only relevant if A != A3):

- [ ] **B1. Yes — accept NonFreeNet label** (we list on F-Droid with the label; users see it during install)
- [ ] **B2. Avoid label by gating cloud AI behind a build flag** (F-Droid build = no cloud AI by default, only available via "Cloud AI" plugin pulled from elsewhere; preserves "free as in libre" status)
- [ ] **B3. Skip F-Droid; ship via GitHub Releases only** (loses the F-Droid trust badge but avoids the label entirely)

**Answer**: <pick B1 / B2 / B3 / N/A if A=A3>

**Rationale**: <…>

---

## Question C — Which cloud provider(s) for v1?

**Context**: Plan currently names Anthropic (Haiku for inline, Sonnet for agent). Adding more providers multiplies BYOK UX complexity and provider-rate-limit handling.

**Pick one** (only relevant if A != A3):

- [ ] **C1. Anthropic only** (Haiku + Sonnet; matches plan)
- [ ] **C2. Anthropic + OpenAI** (Haiku/Sonnet + GPT-4o-mini/GPT-4o; doubles BYOK config UX, doubles SDK dep size)
- [ ] **C3. Provider-agnostic via OpenAI-compatible API** (user picks any compatible endpoint: Anthropic, OpenAI, OpenRouter, self-hosted; max flexibility, max UX complexity)

**Answer**: <pick C1 / C2 / C3 / N/A if A=A3>

**Rationale**: <…>

---

## Question D — If A=A3, is M6 entirely cut from v1?

**Context**: Question A=A3 (defer AI to v2) means M6 (8-10 weeks) is dropped. Total v1 timeline shrinks to M0-M5 (~13-18 months → ~11-15 months). M6 returns in v2 as a feature release.

**Pick one** (only relevant if A=A3):

- [ ] **D1. Yes — M6 entirely deferred to v2; v1 ships at end of M5** (commit to a leaner v1)
- [ ] **D2. M6 partial — only AI agent panel UI shell ships in v1, no actual API call** (~2-3 weeks of M6 retained for UI scaffolding so v2 can drop in API integration; some sunk-cost protection)

**Answer**: <pick D1 / D2 / N/A if A!=A3>

---

## Question E — Companion-mode retreat trigger

**Context**: ADR alternative #6 ("Warp Companion: phone pairs to desktop Warp via SSH/Drive") was rejected as a v1 option but kept as a documented retreat path. We need a precise trigger condition.

**Pick one or write your own**:

- [ ] **E1. Trigger if M0 Vulkan spike fails on 2 of 3 devices** (frame-recovery p95 > 200ms on S24 Ultra + S21+; or any device crashes >5% of cycles)
- [ ] **E2. Trigger if M2a (Layer 1 4 hand-written areas) exceeds 8 weeks** (vs estimated 4 weeks; doubled-budget signal)
- [ ] **E3. Trigger if cumulative budget overrun >50% by end of M3** (i.e., M0+M1+M2+M3 spent more than 50% over estimate)
- [ ] **E4. Other**: <write your own>

**Answer**: <pick E1 / E2 / E3 / E4>

**Reasoning for trigger choice**: <…>

---

## Bound decisions cascade

Once answered, this gate propagates:

1. **A → M6 effort estimate** (A1: full ~8-10w, A2: full ~8-10w, A3: 0)
2. **B → F-Droid metadata** (NonFreeNet label yes/no/build-flag-gated)
3. **C → SDK deps** (Anthropic only / +OpenAI / OpenAI-compat shim)
4. **D → v1 release timeline** (M0-M6 vs M0-M5)
5. **E → ongoing watch** during M0-M3; if trigger fires, plan halts and Companion path becomes live

The plan's `Decision C` table will be auto-amended to reflect A's pick. M6 task table will be revised per A+D. F-Droid metadata stub for v1 commit will be authored per B.

---

## Sign-off

**User**: ____________________
**Date**: __________
**Confirmed**: I have read the plan section 256-266 (Tension 3 questions origin), reviewed the Pre-mortem Scenario A/B/C, and answer above represents my v1 commitment.

**Notes** (optional, anything the future-me should know): <…>
