# Layer/container decomposition of harna-core

harna-core src/ is organized exactly like harna asks users to organize theirs: schema/ (memory layer: manifest, rules, store, migrate), search/ (code-map), enforcement/ (conformance), generation/ (structure templates), composition/ (onboarding compound). Containers never import each other sideways; composition/onboarding reaches down into schema and search. Rationale: dogfooding — the tool must demonstrate the architecture it enforces.
