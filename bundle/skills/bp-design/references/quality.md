# Quality

Load this file with each system design and each container design. Load it for any design that names a characteristic, and for the quality goals. It produces two outputs: the `## Quality goals` section and the check blocks in `## Checks`.

The utility tree, the six-part scenario and the ranking come from Bass, Clements and Kazman, *Software Architecture in Practice*. The fitness functions and their classes come from Ford, Parsons, Kua and Sadalage, *Building Evolutionary Architectures*. The arc42 template holds the quality goals as one of its sections.

## Procedure

1. Take the top characteristics from `references/system.md`, step 5. For a container, take the characteristics of its quantum.
2. Build a utility tree as a nested list: quality, then refinement, then scenario. Each leaf is one scenario.
3. Write each scenario in six parts: source, stimulus, environment, artifact, response and response measure.
4. Make each measure a number with a unit. Add a percentile when the runs repeat. Replace an adjective such as "fast" or "secure" with a number.
5. Run the measurement now. Write the value in a Today column, and cite the run or the analysis file. A scenario with no measurement today is open, not met.
6. Propose a threshold from the Today value. The human sets the threshold.
7. Propose a rank for each scenario by value and difficulty: high, medium or low. The human sets the rank.
8. For each top scenario, name the decision that serves it. A scenario that no decision serves goes to `## Open`.
9. Write a fitness function for each top characteristic and each structural decision. A fitness function is an objective test that keeps the characteristic true as the code changes.
10. Classify each fitness function on three axes. Atomic tests one aspect; holistic tests several. Triggered runs on an event; continual runs always. Static has a fixed threshold; dynamic has a threshold that depends on context.
11. Write each triggered fitness function as a check block in `## Checks`. The kind follows the shape of the test:
    - A threshold is a `scenario` block with `measure` and `max`. Its command prints the measure alone on its last line.
    - A layering rule or a permitted connection is a `deps` block.
    - An architecture test is a `shell` block that names the test.
12. Write each continual or dynamic fitness function as prose with its measure. It does not run at a commit. When a commit can sample it, also write a block with `severity=warn`.
13. Test each fitness function. Seed one violation, and confirm that the block fails. Then remove the seed.

The quality goals table has one row for each scenario. Its columns are attribute, source and stimulus, environment and artifact, response and measure, Today, and value and difficulty.

This block measures one threshold:

```check scenario {name=p95-latency measure=ms max=500}
./scripts/p95-latency.sh  # It prints the p95 in ms.
```

## Diagram

The utility tree and the characteristics radar are not among the five diagram types. Keep the tree as a nested list and the scenarios as a table. This reference draws no diagram.

## Gate questions

- Does each top characteristic have at least one six-part scenario?
- Is each response measure a number with a unit?
- Does each repeated measure give a percentile?
- Does each scenario show a Today value with its source?
- Did the human set each threshold?
- Does a decision serve each top scenario?
- Does each triggered fitness function have a check block?
- Does each block fail on a seeded violation?
- Does a continual fitness function stay out of a commit check?
