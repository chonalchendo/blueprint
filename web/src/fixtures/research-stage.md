'Uncorrelated analysis engines over one corpus, one verdict shape: sweep, lenses, judge, assessment spine, calibration'

**Scope.** This doc owns the **research stage** — everything between "the cheap screen has ranked the universe" and "the author signs a decision". It is a scoped sub-design under [[projects/mentat/designs/system]] (the umbrella owns the four-role lakehouse, the entity spine, serving and UI), sibling to [[projects/mentat/designs/containers/ingestion-pipeline]]. It covers the corpus the stage reads, the engines that analyse it, the facts they land, the adjudication that weighs them, and the calibration loop that scores the whole mixture. It does **not** re-specify the lakehouse grammar, the entity spine, or the quantitative screen — those are the umbrella's, and this stage consumes them.

**Status.** `active` as a **target** design. Almost none of it is built: the four declared `external.research__*` tables are empty, no `nrm_research__*` model exists, two of four legacy lenses cannot run, and the text corpus covers **10 companies against a 4,804-company investable universe**. Read every component as target-state unless marked "(today)".

## Architectural overview

The research stage is **an ensemble of uncorrelated analysis engines over one shared corpus, landing one comparable verdict shape.**

Its identity comes from the design thread, taken literally: *many uncorrelated mental models, quantitative and qualitative, converging on one question — great company, fair price — with every judgment recorded so the mixture itself improves*. The architecture's job is therefore not "run lenses well". It is to make **adding a genuinely uncorrelated engine cheap**, and to make **every engine's verdict comparable, falsifiable, and scoreable after the fact**. Everything below follows from those two goals.

Engines are a growing population — quantitative screens, ML forecasts, a cheap universe-wide LLM sweep, 7 Powers moat analysis, named-investor lenses, inversion/pre-mortem, base rates, valuation. They are therefore **plugins over one grammar, not components**. Each declares its tier, coverage, cadence, output schema and cost profile; the surrounding machinery — evidence, queue, inference, landing, adjudication, calibration — is written once and shared.

Engines run in two directions. **Expansive** engines widen the top of the funnel (find what we would never have looked at); **restrictive** engines try hard to kill what surfaced. Each one's blind spot is another's focus — which is why the roster must be deliberately uncorrelated rather than merely numerous, and why correlation between engines is a monitored quantity rather than an assumption.

The stage keeps the umbrella's integration rule exactly: research reads `modelled`/`serving` data (per [[projects/mentat/designs/components/research-contract]]), calls model providers, and lands verdicts back as `external.research__*` producer sources that bridge up the full stack. It never writes the layer it read, and it never calls another package in Python.

## Tech stack

Inherited wholesale from [[projects/mentat/designs/system]] (uv workspace, DuckLake on R2, Neon catalog, SQLMesh, Modal crons, cyclopts CLI). The deltas this stage introduces:

- **Inference** — self-hosted LiteLLM proxy for synchronous calls (judge, fast-lane triage, low-volume engines); bulk sweeps go **direct to provider batch APIs** (~50% discount does not broker reliably through proxies, and sweeps are never urgent by design). Both paths write model/token/cost rows to the meta domain. Authorized by [[projects/mentat/decisions/inference.0002-litellm-proxy-direct-provider-batch-apis-are-the-inference-l]].
- **Structured output** — every engine's output is a Pydantic schema; providers are called in strict JSON-schema mode. The schema *is* the checklist (see invariant 4).
- **Bulk corpus** — `defeatbeta/yahoo-finance-data` on Hugging Face, a single parquet per dataset read directly by DuckDB `httpfs`. Licence ODC-BY. Verified 2026-08-15: 238,206 transcripts / 6,465 symbols / FY2005–2027, and 1,242,289 news articles / 8,525 symbols with full paragraph body text; whole-file download ~70s; publication lag T+1.
- **Filing corpus** — EDGAR document fetch driven off `external.edgar_fsds__sub`, which already holds the complete filing index (94,452 10-Ks across 14,023 CIKs, 2004→2026; 290,426 10-Qs). Fetch respects EDGAR's 10 req/s fair-access limit with a descriptive User-Agent.
- **Fast lane** — EDGAR "latest filings" Atom feed (acceptance timestamps to the second) plus the daily index. Free, universe-wide, minutes latency.
- **Predictive models** — gradient-boosted trees over tabular PIT panels, in the new `mentat-quant` package (below). Not part of the first milestones.

## Components and responsibilities

Seven components. Four are new lakehouse/serving surfaces, two are `mentat-research` planes, one is a new package.

**1. The corpus** *(mentat-connectors + mentat-lakehouse)* — the text every engine reads, spine-keyed and bitemporal. Three bulk lands (EDGAR filing documents sectioned into Item 1 / 1A / 7; defeatbeta transcripts; defeatbeta news) normalised into `fct_text__document` and `fct_filings__section`, keyed to `company_id`, never to a ticker string. This is the stage's hard prerequisite and its current hard blocker. *Serves: M-A.*

**2. The evidence plane** *(mentat-lakehouse, `serving`)* — the PIT-filtered bundle an engine is given: `srv_context__company_dossier` (the structured half — quantitative facts with their WHY, per principle 8), `srv_context__company_documents` (ranked text pointers), and `srv_context__engine_queue` (what is due). Evidence assembly is **deterministic SQL owned by the lakehouse, not ad-hoc Python owned by each engine** — today five lens modules each carry their own hand-written query, which is why nothing is spine-keyed and why "cannot-score: missing input" is a per-lens accident. *Serves: M-B.*

**3. The engine registry** *(mentat-research)* — the plugin contract. An engine declares `tier`, `coverage`, `cadence`, `direction`, its Pydantic output schema, its projection into the assessment spine, its model roster and its cost profile. Adding an engine is writing the engine and registering it; it is never editing the judge, the screen, or the UI. *Serves: M-B, M-C, and every later engine.*

**4. The execution plane** *(mentat-research)* — one way to run anything: read the queue, render prompts from versioned templates, dispatch synchronously (LiteLLM) or as a batch (submit → poll → collect), validate output at write, land producer rows, and record token/cost. Replaces today's 735-line `runner.py`, which fuses prompt loading, caching, provider dispatch, four hardcoded row schemas, ensemble collapse and cost estimation into one module and prints cost to stdout rather than landing it. *Serves: M-B.*

**5. The verdict plane** *(mentat-lakehouse)* — every engine says its piece twice. A **rich native fact** in the engine's own shape (`fct_research__lens_run`, `__sweep_run`, `__company_power`, `__inversion_run`, `__valuation_run`, `fct_forecast__company_prediction`) for reading and auditing; and one thin row in **`met_research__company_assessment`** — grain `(company_id, engine, as_of_date)`, carrying verdict, confidence, direction and `evidence_refs` — for the judge, the screen and the backtest. The spine is what lets the roster grow without rewriting every consumer. *Serves: M-B.*

**6. Adjudication** *(mentat-research + mentat-lakehouse)* — the judge (its own frontier model, its own fact `fct_research__judge_run`, never extra lens rows) reads the assessment spine and emits conviction, per-engine weighting, `disagreement_flag`, thesis and key risks, with "no verdict warranted" a legitimate output. Beside it sit the deterministic checklist gates — computed arithmetic the model articulates but never calculates. Where its verdict surfaces is governed by the coverage rule below, not by its being the judge. *Serves: M-C.*

### Where verdicts surface: lane membership follows coverage

A verdict reaches `srv_api__opportunity_screen` as a lane **only if the engine covers the universe**. This is a rule about grain, not about importance.

| engine | coverage | surfaces as |
|---|---|---|
| `screen` | ~8,100 | screen lane **(built)** |
| `forecast` | ~4,800 | screen lane |
| `sweep` | ~4,800 | screen lane |
| `powers`, `lenses`, `inversion`, `valuation` | ~200 | `srv_api__company_research` only |
| `judge` | ~30 | three pointer columns on the screen |

The screen is one row per universe company, FULL-rebuilt, already carrying twelve audits and two staleness walls to keep vintages aligned. Hanging a 30-name payload off it would be 99.6% NULL and would need a third staleness wall for LLM prompt/model vintages — and it would fuse a *breadth ranking surface* with a *depth dossier surface*, two different jobs for two different moments.

Universe-coverage engines are genuine lanes: they are company-grain discovery signals exactly like `revision_momentum`, and the existing `<lane>_rank` / `<lane>_status_reason` vocabulary — including `not_yet_covered` and `stale_at_universe_snapshot` — absorbs them unchanged. Shortlist engines get their own serving surface, `srv_api__company_research`. The judge is the single exception, carrying `judge_conviction`, `judge_as_of` and `disagreement_flag` onto the screen so "show me ranked names the ensemble likes" stays one query — the pointer, never the payload.

**7. The calibration loop** *(mentat-research + mentat-quant)* — golden sets with agent-supplied labels and an owner-audited slice (ruling on issue 0473, 2026-09-04), in-prompt anchor companies, a fixed drift set re-run on schedule to catch silent provider-side model changes, and the replay harness that re-runs engines at historical as-of dates against forward GBP returns. Its output is evidence about **which engines earned their keep**, which is what moves engine weights. *Serves: M-C, and the backtest milestone.*

**New package: `mentat-quant`.** Predictive/ML engines get their own workspace member rather than a `mentat-research` submodule: ML has a different dev loop (train / tune / evaluate / promote, feature stores, model registries) and drags heavy dependencies that the lens image should never carry. It is import-independent like the other data packages and integrates through the lakehouse. `mentat-research` stays the judgement engine. It gets its own contract when the first forecast engine is shaped.

### The engine roster

The registry's population. Engines are added by justifying a *new uncorrelated mental model*, never by adding a variant of one that exists.

| engine | direction | kind | tier | emits |
|---|---|---|---|---|
| `screen` | expansive | SQL | T0 | rank + reason **(built)** |
| `forecast` | expansive | ML | T1 | predicted fundamental change |
| `sweep` | expansive | LLM | T2 | "anything here a numbers screen missed?" |
| `themes` | expansive | LLM | T2/3 | theme exposure + hop count |
| `powers` | restrictive | LLM | T3 | moat claims: benefit + barrier + footprint |
| `lenses` | restrictive | LLM | T3 | five investor verdicts **(partial)** |
| `inversion` | restrictive | LLM | T3 | failure modes → monitorable drivers |
| `base_rates` | restrictive | SQL+LLM | T0/3 | reference class + why this case beats it |
| `valuation` | restrictive | SQL+LLM | T3 | scenarios + implied expectations |
| `judge` | adjudicate | LLM | T4 | conviction, disagreement, thesis |
| `checklist` | adjudicate | SQL | T0 | computed pass/fail gates |

`forecast` targets **fundamentals** (margin inflection, distress, accrual reversal), never returns. Price action is admissible to engines as *evidence and timing context* (drawdown depth, relative strength, forced-selling signature); technical **trading rules** remain a named product non-goal.

### The tier economy

Tiers exist because the scarce resource at the bottom is **the author's attention**, not money. Each tier's only job is to earn the next tier's attention.

| tier | who runs it | coverage | cadence | cost/sweep |
|---|---|---|---|---|
| T0 | deterministic SQL | ~8,100 | daily | ~£0 |
| T1 | ML forecasts | ~4,800 | weekly | ~£0 (own compute) |
| T2 | cheap LLM sweep | ~4,800 | quarterly / on-filing | ~£25–40 batched |
| T3 | deep structured engines | ~200 | on-event | ~£1/name |
| T4 | frontier judge | ~30 | on-shortlist | ~£2/name |
| T5 | the author | ~5 | — | the real budget |

The T2 number is the one that makes the whole design work: reading every in-universe company's filings — including the micro-caps no analyst covers — costs tens of pounds per sweep. That is the coverage-vacuum edge turned into a line item.

**Measured vs estimated.** Coverage counts are measured against prod on 2026-08-15 (8,109 spine / 4,804 in-universe / 1,718 ranked; corpus figures in Tech stack). Every **cost** figure is an estimate resting on an assumed ~40k input tokens per sectioned filing at cheap-tier batch pricing, and none has been observed. The first sweep's actual cost is a finding to record, not a number to defend — and no gate anywhere in this design may be calibrated against these figures.

## Code design

The exemplar is **`mentat-connectors`**, in this repo. Its base module states the goal outright: *"adding a source is a new Dataset subclass + one union member + a conf file — no CLI wiring."* That is exactly the registry property this stage needs, already proven over ~35 connectors, and it beats importing a pattern from outside. `Dataset = Fetcher + Output` becomes `Engine = Evidence + Inference + Output`.

| `mentat-connectors` | `mentat-research` |
|---|---|
| `base.Dataset` — `run()` is the sole writer | `base.Engine` — `run()` is the sole writer |
| `_collect_rows()` — the subclass's only job | `_analyse()` — the subclass's only job |
| `row_schema` ClassVar — declared write schema | `verdict_schema` ClassVar — declared output schema |
| `CoverageGuard` — free to every Dataset | `CalibrationGuard` — free to every Engine |
| `DatasetKind` discriminated union + `load_dataset` | `EngineKind` discriminated union + `load_engine` |
| `FetcherKind` — pluggable transport axis | `InferenceKind` — pluggable execution axis |
| `schemas/` — per-source row schemas | `schemas/` — per-engine verdict schemas |
| `confs/*.yaml` — cadence declared per source | `confs/*.yaml` — tier, coverage, roster per engine |

```
mentat_research/
  engines/
    base.py        Engine ABC — run() is the sole writer
    kind.py        EngineKind union + load_engine(conf)
    sweep.py  lenses.py  powers.py  inversion.py  judge.py
  inference/
    base.py  kind.py     InferenceKind union
    litellm.py           synchronous
    batch.py             submit → poll → collect
    deterministic.py     engines that call no model at all
  evidence/        dossier + document binding, company_id-keyed
  schemas/         one verdict schema per engine
  producers/       external.research__* landing
  eval/            golden · drift · promotion gate
  cli/
confs/{engine}.yaml
prompts/{pipeline}/{investor}/v{N}.txt
```

Two properties are lifted deliberately rather than reinvented.

**`InferenceKind` carries a `deterministic` member.** That is what lets `screen`, `checklist` and `base_rates` be ordinary engines rather than special cases — they have no model roster, and `base.Engine` does not care. Without it, half the roster would need a parallel code path and the registry's value would evaporate.

**`CalibrationGuard` is `CoverageGuard` ported to inference.** Connectors learned from the 2026-07-24 yfinance throttle (issue 0148) that *a throttled client swallows its own drops and reports success with fewer rows* — so the guard bands on requested-vs-returned proportion, never on exception counts. The LLM failure mode has the same shape: **a degraded model returns perfectly valid JSON containing junk.** So the guard bands on the *distribution* of a run — null-verdict rate, inter-model agreement, verdict entropy against a stored baseline — and refuses to land a sweep whose shape has collapsed. Thirteen sources inherited one coverage convention instead of hand-rolling thirteen; every engine should inherit one calibration convention the same way.

**A budget guard belongs to the execution plane, not the engine.** Today `max_cost` is a per-lens config field, which cannot see a bug that re-runs the universe daily instead of quarterly — at T2 rates that is the difference between tens of pounds a quarter and tens of pounds a day. The spend ceiling is enforced once, over all engines, at submit time.

### The `sweep` engine, specified

The first engine carries the most design risk, because "find anything interesting a numbers screen would miss" is precisely the vague brief that produces plausible, non-discriminating output. Per invariant 10 it must **extract, not rate**: the sweep returns a set of typed findings, each with a verbatim quote and its document location, or the empty set.

Candidate finding types — chosen because each is (a) textually explicit, (b) missing from the quantitative screen, and (c) checkable against a filing by a human in seconds: customer/supplier concentration, related-party transactions, going-concern language, auditor change or disagreement, material weakness in internal controls, litigation with quantified exposure, dilution and toxic financing terms, segment restatement or redefinition, covenant proximity, and key-person dependence.

The *score* is then a deterministic rule over validated findings — count, weight by type, or simply "any finding present" — computed in SQL, never asked of the model. A finding whose quote does not appear in its cited document is rejected at write.

## Data flow

The critical path is one full sweep cycle: corpus → evidence → engines → verdicts → judge → author. Every baton-pass between packages is data landing in the lakehouse.

*Diagram [`mentat-research-data-flow`](/projects/mentat/designs/containers/research-stage/diagrams/mentat-research-data-flow.html) renders on this page's canvas in anvil ui.*

Two properties worth naming. **Submit runs daily even though nothing is usually due** — the queue is a SQL count, so an empty day costs nothing, and a newly filed 10-K is analysed within about a day without any event machinery. And **the judge runs one transform cycle behind the engines**, deliberately: it is due when a company's assessment set carries a newer vintage than the `evidence_refs` its last verdict consumed. That is self-healing and needs no orchestration coupling.

### The fast lane

A second, thin path for material disclosures, plugged into the same registry. Resolves the EOD non-goal ruling left open by the design thread: *no real-time/intraday **market** data — but filing/disclosure **events** may be detected at minutes latency to trigger targeted re-analysis*.

*Diagram [`mentat-research-fast-lane`](/projects/mentat/designs/containers/research-stage/diagrams/mentat-research-fast-lane.html) renders on this page's canvas in anvil ui.*

Edge-filtering kills ~99% of events before any model runs, so the lane is nearly free. Its edge is the **thin-coverage end of the universe** — being hours early on an uncovered micro-cap's 8-K is differentiated because nobody is reading it at all; being minutes early on a mega-cap is a latency game the product design refuses. 8-K item numbers are structured signal, not prompt-inferred (4.02 non-reliance, 5.02 executive departure, 1.01 material agreement, 2.02 earnings).

## Boundaries and integration points

*Diagram [`mentat-research-boundaries`](/projects/mentat/designs/containers/research-stage/diagrams/mentat-research-boundaries.html) renders on this page's canvas in anvil ui.*

`mentat-research` and `mentat-quant` read `serving.*`, call model providers, and land `external.*`. They import neither each other nor connectors, lakehouse or serving — `import-linter` enforces it, and the lakehouse remains the bus.

## Key invariants

1. **Every engine is a pure function of `(company_id, as_of_date, version)` over point-in-time data.** No engine reads anything with a `knowledge_date` later than its `as_of_date`. This is the single property that makes replay-based backtesting possible at all, and it cannot be retrofitted — the same reason every LLM verdict stamps `model_cutoff_date` at write time. It is necessary but, for LLM engines, not sufficient (see the replay-contamination risk below). If it broke, every calibration result would be contaminated by lookahead and the mixture could never be tuned on evidence.

2. **Every engine may return "nothing to say", and that is a success.** Null verdicts are expected output, declared in every output schema and required by every prompt. A framework that finds its pattern everywhere is noise wearing a rating. If it broke, the ensemble would manufacture conviction and the disagreement signal the judge exists to read would be destroyed.

3. **Every engine says its piece twice: a rich native fact and one assessment-spine row.** The native fact is the audit trail; the spine row `(company_id, engine, as_of_date, verdict, confidence, direction, evidence_refs)` is what the judge, the screen and the backtest consume. If it broke, adding the tenth engine would mean editing the judge, the screen and the UI — and the roster would stop growing.

4. **Structure is validated at write and never argued with.** Output is a Pydantic schema, dispatched in strict JSON-schema mode; a 7 Powers claim missing its barrier evidence is *rejected*, not debated. The checklist is the shape of what a model is allowed to write, not a document it reads. If it broke, the model would perform the checklist instead of passing it.

5. **Arithmetic is never delegated to a model.** Thresholds, gates, base rates, margins of safety and position math are computed deterministically; the model articulates them. If it broke, the system's numbers would inherit LLM variance and stop being auditable.

6. **No name reaches a tier without earning it at the tier below.** Breadth at the top, depth at the bottom — the umbrella's cost funnel, extended: T2 admits the whole universe cheaply, T3+ admits only what T0–T2 flagged. The fast lane may bypass the *cost* tiers on a material disclosure, but never the author.

7. **Uncorrelatedness is measured, not assumed.** Pairwise engine agreement is a standing monitor; two engines that agree almost always are one engine with redundant cost. *Honest caveat:* with a ~200-name shortlist and few engines, this monitor is statistically meaningless for its first year — until then it is a design discipline and a recorded quantity, not a gate. It must not be dressed up as one. If it broke, the ensemble would become an expensive echo and its apparent consensus would mean nothing.

8. **Engine weights are earned, never hand-set.** All engines start equal and *stay* equal until the decision journal and replay harness have enough observations to move them — which will be a long time. The invariant is that nothing else may move them, not that they will move soon. If it broke, the mixture would encode today's prejudices permanently and the compounding loop would stop compounding.

10. **No engine emits a number it did not compute.** A model extracts structured evidence; the system derives the score from validated claims. Where a rating is unavoidable it is a short ordinal scale pinned by anchor companies inside the versioned prompt — never a wide cardinal range, and never a self-reported confidence. See *Scoring discipline* below. If it broke, the system's central quantities would be unfalsifiable model opinion wearing the costume of measurement, and no amount of downstream calibration could recover them.

9. **The fast lane fires pre-registered criteria, never raw alerts.** An alert says *which pre-committed condition fired, when it was committed, and on what evidence* — it never says "something happened, react". The written policy is a temperament prosthetic; an alert stream that trains reflex would defeat the thing it is meant to protect.

## Scoring discipline

Asking a language model for a number is the stage's most seductive failure mode, and the current code commits it twice: `sentiment_score: int = Field(ge=-100, le=100)` — a 201-point scale no model can reproduce, where the distinction between 62 and 67 is pure noise — and a Marks prompt instructing *"default upward under ambiguity… `very_strong`/`strong` is preferred over `adequate`"*, which is an instruction to bias a subjective scale and makes the output unfalsifiable by construction. Both are retired.

The rules, in descending order of how much they buy:

1. **Extract, don't rate.** The model emits structured evidence; the system computes the number from validated claims. 7 Powers is the template: not *"rate the moat 1–10"* but *benefit evidence, barrier evidence, footprint metric* per power — each admissible or rejected — with the score a deterministic rule over what survived. This is invariant 5 applied to scoring, and it is the whole answer wherever it can be made to fit.
2. **Ordinal, never cardinal.** Where a rating is genuinely unavoidable: at most five ordered categories, with two or three anchor companies pinned inside the versioned prompt so the scale is fixed in-context rather than re-invented per call. Wide numeric ranges are banned outright.
3. **Comparative beats absolute.** Models are markedly more stable at *"is A stronger than B"* than at *"rate A"*. Pairwise comparison within a cohort, resolved into a ranking by a deterministic method (Bradley–Terry or equivalent), is the highest-fidelity option available. It is also the most expensive, so it is reserved for shortlist grain — but the seam is designed now rather than retrofitted.
4. **Confidence comes from agreement, never self-report.** A model's stated confidence is close to worthless; agreement across three cross-provider models is a real signal. This is why the verdict plane lands one row per model and computes consensus in SQL rather than collapsing to a winner in Python.
5. **Calibration proves the scale.** The golden set answers the only question that matters — does this scale separate known winners from known blowups, judged hindsight-free? An engine that cannot do so is noise and never reaches the screen, however articulate its prose.

Rules 1 and 2 are the design; rule 3 is the escape hatch when they do not suffice; rules 4 and 5 are how we know any of it is working.

## Why this shape

The design thread's own identity paragraph is the brief: *many uncorrelated mental models… with every judgment recorded so the mixture itself improves*. Read carefully, that sentence contains three architectural demands, and the shape above is the minimum that satisfies all three.

*Many models* means the roster grows — so engines are plugins over one grammar rather than components, and the assessment spine exists so that growth costs nothing downstream. The alternative, a bespoke fact per engine consumed by bespoke code, is exactly today's shape: four producer tables, four schemas, and consequently no judge, no screen lane and no consumer at all. The spine's cost — every engine must map into a shared verdict vocabulary, awkward for engines that emit a number rather than an opinion — is worth paying once per engine to avoid paying it in every consumer.

*Uncorrelated* means diversity is the product, not a by-product. That is why the roster spans expansive and restrictive, structural and statistical and adversarial, SQL and ML and LLM — and why invariant 7 makes correlation a monitored quantity. A roster of five engines that agree is worse than two that do not, and nothing in the system would have told us so without measuring it.

*Every judgment recorded so the mixture improves* means calibration is load-bearing infrastructure, not a later nicety — and calibration is only possible if engines are replayable, which is invariant 1. This is why the PIT discipline and `model_cutoff_date` are non-negotiable from the first engine: the thread's own warning is that they are cheap now and impossible to retrofit.

The cheap universe-wide sweep is the design's sharpest new idea and it is an economic argument, not a technical one. The structural edge is a coverage vacuum — thousands of small and micro caps that no analyst reads, for reasons that do not decay (price impact, materiality, disclosure friction). Until now, "read everything" was the one thing a solo investor could not do. At roughly £25–40 per full-universe pass, it is now the cheapest tier in the system, and it inverts the classic funnel: the LLM is no longer only the expensive stage at the bottom, it is also a breadth instrument at the top that finds what a numbers screen structurally cannot see.

Evidence assembly sits in the lakehouse rather than in each engine because of what the current code demonstrates: five lens modules each carrying their own SQL is why nothing is keyed to `company_id`, why two lenses silently cannot run, and why "cannot-score" is a per-lens accident instead of one enforced rule. Moving it to `srv_context__*` makes prompt inputs deterministic, versioned, auditable and testable in SQL — and lets `mentat-research` honour its contract of reading serving data only.

Consensus moves from Python into SQL for the same reason. Today `ensemble.majority_vote` picks one winner by rating and discards the other models' outputs, which makes the declared grain `(company, lens, model_ver, prompt_ver, run_date)` unachievable and starves the judge of disagreement. Landing one row per model and computing agreement in a met makes nothing lost, `no_consensus` a legible `cannot-score` state, and the ensemble auditable.

`mentat-quant` is a separate package rather than a research submodule because ML's dev loop and dependency weight differ from prompt-and-parse, and the workspace split has to keep mapping onto the image split. The lens image should never carry a training stack.

## Risks

- **The corpus is the whole plan's critical path.** Today the stage can read 10 companies against a 4,804-company universe; every engine above T0 is blocked until M-A lands. Signal: engine work starts before the corpus is spine-keyed and universe-wide. Mitigation — M-A ships alone and is verified by row counts against the universe, not by a green test suite.
- **A single third-party mirror carries two of three corpora.** defeatbeta is a Hugging Face mirror of Yahoo data with no SLA; it could vanish. Mitigation is structural rather than contractual: it is *bulk*, so the first download lands permanently in `external` and we own the copy whatever happens upstream. Signal: a scheduled land 404s. Residual: no primary source exists for transcripts, so a gap in new quarters has no free substitute.
- **Ticker-keyed sources meet a company_id spine.** defeatbeta keys on symbol; 6,465 symbols must map onto the crosswalk, and the honest coverage number will move when they do. Signal: `fct_meta__unmatched_symbol` grows after the transcript land.
- **The sweep finds nothing, expensively.** A universe-wide cheap pass may return plausible flags that do not discriminate — the lens-drift failure at larger scale. Mitigation — the sweep is gated by the same golden-set promotion rule as every other engine, and its flags must beat a null baseline before reaching the screen. Signal: flag rate is uniform across the universe.
- **The assessment spine flattens what mattered.** Compressing a rich verdict into `(verdict, confidence, direction)` may discard the distinction that made the engine worth running. Signal: the judge's verdicts stop tracking what a human reading the native facts would conclude. Mitigation — `evidence_refs` always resolve to the native fact; the spine is a routing layer, never the record.
- **Batch inference is asynchronous and can silently stall.** Submit/collect across provider batch APIs introduces a state machine where the failure mode is silence, not error. Mitigation — open batches are tracked with an age bound and surface in freshness monitoring. Signal: the queue is non-empty for days with no landed rows.
- **The fast lane trains a reflex.** Even with invariant 9, a stream of alerts can erode the pre-commitment discipline the policy exists to enforce. Signal: overrides of pre-registered kill criteria become common in the decision journal — which is itself the measurement, and the reason the journal must precede the lane.
- **LLM engines are barely backtestable, and no invariant fixes it.** PIT discipline controls what an engine is *given*; it cannot control what the model already *knows*. A frontier model replayed at a 2015 as-of date knows how 2015 resolved, so its verdict is contaminated no matter how cleanly the evidence is filtered. `model_cutoff_date` makes this filterable rather than invisible, but the honest consequence is stark: **historical replay of an LLM engine is only valid for as-of dates after that model's cutoff** — so meaningful LLM calibration accrues forward from today and cannot be manufactured from history. Deterministic and ML engines have no such limit and are replayable over the full PIT record. Mitigation is not technical: forward paper-tracking is the primary evidence path for LLM engines, and the backtest harness is primarily evidence about T0/T1. Signal: a calibration result that looks too good on pre-cutoff dates.

- **Engine sprawl.** Every new framework feels uncorrelated to its author. Mitigation — invariant 7 makes the claim testable, and an engine that fails it is removed rather than tuned.

## Divergences from the frozen model inventory

The product-design's 2026-07-20 addendum froze a target model inventory and declares itself living — models shaped later "follow the grammar and get added to this file". This design introduces the following, which must be folded back:

| model | status | note |
|---|---|---|
| `met_research__company_assessment` | **new** | the assessment spine, grain `(company_id, engine, as_of_date)` |
| `fct_research__sweep_run` | **new** | typed findings per company per document |
| `fct_research__inversion_run` | **new** | failure modes mapped to monitorable drivers |
| `fct_research__company_power` | new (from thread 0002 §6) | 7 Powers, admission rule enforced at write |
| `srv_api__company_research` | **new** | the shortlist depth surface |
| `srv_context__engine_queue` | **new** | the due-list, as data rather than Python |
| `fct_forecast__company_prediction` | **new** | `mentat-quant`'s output; new `forecast` domain |
| `fct_text__document` | **clarified** | carries extracted text itself, not just metadata + extracted claims -- `mentat-research` reads modelled/serving, so a raw-text-stays-external split would force every engine to reach past its own contract for a prompt input; the inventory's "raw text stays external" line is about document *artifacts* (HTML, zips), not extracted text (mentat.0401) |
| `fct_meta__lens_eval` | **rename** → `fct_meta__engine_eval` | engines are plural; gains an `eval_kind` (golden \| drift) |

**One grammar question to settle at shaping time, not here.** `met_research__company_assessment` carries `engine` as a grain column, so it is one row per *(entity, engine, period)* rather than the met grammar's *(entity, period)*. That is correct under long-over-wide — engine is data, not structure — but it stretches the stated met definition and should be checked against `grain_lint.py` before the model is written, in the same spirit as the `bridge_` prefix conflict thread 0002 §10 flagged. If the lint objects, the answer is to teach the lint, not to widen the table.

## Milestone mapping

| milestone | payload | components |
|---|---|---|
| **M-A · the corpus** | defeatbeta transcripts + news bulk lands; EDGAR document fetch off the existing `edgar_fsds__sub` index; Item 1 / 1A / 7 sectioning; `fct_text__document` + `fct_filings__section` spine-keyed. No AI. Ships alone: 4,800 companies become readable. | 1 |
| **M-B · verdict plane + first engine** | evidence plane (`srv_context__*`); inference layer (sync + direct batch) with cost landing in meta; engine registry; assessment spine; bridge-up to the screen; **`sweep`** as the first engine, universe-wide. Ships alone: the screen gains a "something here?" lane. | 2, 3, 4, 5 |
| **M-C · interrogation tier** | `lenses` re-seated on `company_id` with one row per model and SQL consensus; the judge; golden sets, anchors, drift set, promotion gate. | 6, 7 |
| **M-D · fast lane** | EDGAR Atom poller, 8-K item taxonomy, cheap triage, kill-criteria alerting. Needs M-B's plane and pre-registered criteria from the portfolio work. | 3, 6 |
| later | `powers`, `inversion`, `base_rates`, `themes`, `valuation`; `forecast` + `mentat-quant` once `srv_features__*` exists; replay harness. | 3, 7 |

`sweep` precedes `lenses` despite lenses half-existing: one prompt, one schema, one corpus, universe-wide exercises the registry, the spine, the batch path, cost accounting and the bridge-up with the fewest moving parts — and delivers the capability nothing else on the market has. Lenses bring five schemas, ensemble consensus, a judge and an eval harness, all of which then land on proven rails.

## Deliberately not designed here

- **Per-company cycle positioning** — a product conviction with no operational definition; inventing one at design altitude would be guessing. Remains an open research thread, and gates the `cycle` engine only.
- **Relationships and themes as a domain** — `fct_relationships__company_edge` and the theme layer are their own domain and milestone; the `themes` engine is registered here but not specified.
- **Section deltas** — comparing a risk-factor section against its predecessor is a natural M-A+ extension once the corpus is sectioned; noted, not scoped.
- **The PM agent** — blocked on the owner-authored written investment policy, and belongs to the portfolio milestone.
