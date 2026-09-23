lab_main.swift is the `main.swift` of the copy of the elk-swift lab that
produced group_a_chain_golden.txt, group_a_components_golden.txt and
network_simplex_golden.txt (modes `ns`, `chain`, `comp`). To rebuild it:
copy target/elklab (crates/elk/tools/elklab.sh), replace Sources/lab/main.swift
with this file, add `package init() {}` to NGraph,
EdgeAndLayerConstraintEdgeReverser and ComponentsProcessor, and
`swift build -c release`.
