#!/usr/bin/env python3
"""Build a lab scenario file = helper section of graded.cpp + candidate cases + main."""
import sys
graded, cases, out = sys.argv[1:4]
g = open(graded).read()
cut = g.index("// =========================================================================\n// Family A")
head = g[:cut]
main = "\nint main(int argc, char** argv) { return harness::main_impl(argc, argv); }\n"
open(out, "w").write(head + open(cases).read() + main)
print("wrote", out)
