#!/usr/bin/env python3
"""Instance directories and map documents shared between the Python model and
the Rust crate (author-side)."""
import json, os, subprocess, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import model
from model import Map, Band


def write_instance(d, prob, header):
    os.makedirs(d, exist_ok=True)
    fasta, enz, dig = model.problem_to_files(prob, header)
    open(os.path.join(d, "backbone.fasta"), "w").write(fasta)
    json.dump(enz, open(os.path.join(d, "enzymes.json"), "w"), indent=1)
    json.dump(dig, open(os.path.join(d, "digests.json"), "w"), indent=1)


def read_instance(d):
    return model.load_problem(os.path.join(d, "backbone.fasta"), json.load(open(os.path.join(d, "enzymes.json"))), json.load(open(os.path.join(d, "digests.json"))))


def read_map(path):
    doc = json.load(open(path))
    return Map(doc["cloning_site"], doc["insert_length"], {e: list(v) for e, v in doc["cuts"].items()}), {hid: [Band(b["length"], b["multiplicity"]) for b in bands] for hid, bands in doc["held_out"].items()}


def run_binary(binary, d, out, timeout=600):
    r = subprocess.run([binary, d, out], capture_output=True, text=True, timeout=timeout)
    return r.returncode, r.stdout, r.stderr
