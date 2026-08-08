#!/usr/bin/env python3
"""Generate the wonder, nation-power, and rare-resource effect-site ledger.

The scanner is intentionally pinned to the shipped ``riseofnations.exe`` and the
repository's recovered PDB metadata.  It finds explicit calls to the four
LeaderData predicates plus the compiler-inlined ``has_rare`` bit tests, then
recovers Constants[] reads in each guarded block.

Usage:
    python3 tools/effects-ledger.py [OUTPUT]
    python3 tools/effects-ledger.py --check [OUTPUT]

The default OUTPUT is ``schema/effects.json``.  ``--check`` never writes: it
regenerates in memory, compares bytes, and exits non-zero on drift.
"""

from __future__ import annotations

import argparse
import difflib
import json
import re
import sys
from pathlib import Path
from typing import Any

try:
    import pefile
    from capstone import CS_AC_READ, CS_ARCH_X86, CS_MODE_32, Cs
    from capstone.x86 import (
        X86_INS_ADD,
        X86_INS_AND,
        X86_INS_CALL,
        X86_INS_CMP,
        X86_INS_DIV,
        X86_INS_IDIV,
        X86_INS_IMUL,
        X86_INS_JE,
        X86_INS_JNE,
        X86_INS_LEA,
        X86_INS_MOV,
        X86_INS_MUL,
        X86_INS_NEG,
        X86_INS_NOP,
        X86_INS_OR,
        X86_INS_PUSH,
        X86_INS_SAR,
        X86_INS_SHL,
        X86_INS_SUB,
        X86_INS_TEST,
        X86_INS_XOR,
        X86_OP_IMM,
        X86_OP_MEM,
        X86_OP_REG,
    )
except ModuleNotFoundError as exc:  # pragma: no cover - environment diagnostic
    raise SystemExit(
        f"missing Python dependency {exc.name!r}; install with "
        "`python3 -m pip install pefile capstone`"
    ) from exc


ROOT = Path(__file__).resolve().parents[1]
EXPECTED_COUNTS = {
    "total": 475,
    "by_predicate": {"has_rare": 54, "has_tribe_bonus": 320, "has_wonder": 101},
    "by_kind": {"call": 428, "inline": 47},
}

EXE = ROOT / "ron-bin" / "riseofnations.exe"
SYMTAB = ROOT / "re" / "symtab.json"
PDB_TYPES = ROOT / "schema" / "pdb-types.json"
RULES_CONSTANTS = ROOT / "docs" / "derivation" / "rules-constants.json"
SYMBOLS = ROOT / "schema" / "symbols.json"
DEFAULT_OUTPUT = ROOT / "schema" / "effects.json"

PREDS = {
    0x006EBC10: "has_wonder",
    0x006E1370: "has_tribe_bonus",
    0x006E0770: "has_rare",
    0x006D9320: "has_rare_conquest",
}
CONST_PTRS = (0x00C061E4, 0x00C061F0)
RAREC_PAY = 0x6DCC
LEADERS = 0x00E3A390

TRIBES = (
    "aztecs maya inca bantu nubians greeks romans egyptians turks spanish french "
    "british germans russians chinese japanese koreans mongols iroquois lakota "
    "americans indians dutch persians"
).split()

DOM = {
    "has_wonder": range(526, 543),
    "has_tribe_bonus": range(0, 24),
    "has_rare": range(6, 50),
    "has_rare_conquest": range(6, 50),
}

FULL_REG = {
    "al": "eax",
    "ah": "eax",
    "ax": "eax",
    "bl": "ebx",
    "bh": "ebx",
    "bx": "ebx",
    "cl": "ecx",
    "ch": "ecx",
    "cx": "ecx",
    "dl": "edx",
    "dh": "edx",
    "dx": "edx",
    "si": "esi",
    "di": "edi",
    "bp": "ebp",
    "sp": "esp",
}

ARITH = {
    X86_INS_IMUL: "mul",
    X86_INS_ADD: "add",
    X86_INS_SUB: "sub",
    X86_INS_IDIV: "div",
    X86_INS_MUL: "mul",
    X86_INS_DIV: "div",
    X86_INS_SHL: "shl",
    X86_INS_SAR: "sar",
    X86_INS_OR: "or",
    X86_INS_AND: "and",
    X86_INS_NEG: "neg",
    X86_INS_LEA: "lea",
}

DYNAMIC_NOTES = {
    0x006DA479: (
        "argument is dynamic: push dword ptr [ebp + 8]; this is the "
        "LeaderData::has_unbuilt_wonder wrapper parameter (wonder TypeIndex "
        "526..542). Its three direct callers pass SUPERCOLLIDER (541), "
        "SPACEPROGRAM (542), or forward team_has_unbuilt_wonder's parameter"
    ),
    0x009EA093: (
        "argument is dynamic: push eax; ScenarioFuncSet::has_rare_resource "
        "resolves the script String through ScenarioFuncSet::get_type_index, "
        "returns true directly for resolved values 0..5, and calls has_rare "
        "for values 6..49"
    ),
}


def load_json(path: Path) -> Any:
    try:
        with path.open(encoding="utf-8") as handle:
            return json.load(handle)
    except FileNotFoundError as exc:
        raise SystemExit(f"required input is missing: {path}") from exc


def normalize_register(name: str | None) -> str | None:
    return FULL_REG.get(name, name) if name else None


def build_ledger() -> dict[str, Any]:
    pe = pefile.PE(str(EXE), fast_load=True)
    image_base = pe.OPTIONAL_HEADER.ImageBase
    sections = {
        section.Name.rstrip(b"\x00").decode(): (
            image_base + section.VirtualAddress,
            section.get_data(),
        )
        for section in pe.sections
    }
    text_lo, text = sections[".text"]
    text_hi = text_lo + len(text)

    symtab = load_json(SYMTAB)
    procs = sorted(
        {proc["va"]: proc for proc in symtab["procs"] if proc["size"]}.values(),
        key=lambda proc: proc["va"],
    )

    pdb_types = load_json(PDB_TYPES)
    type_names: dict[int, str] = {}
    for value in pdb_types["enums"]["TypeIndex"]["values"]:
        match = re.search(r"\((-?\d+)\)", value["value"])
        if match:
            type_names.setdefault(int(match.group(1)), value["name"])

    # rules-constants.json has recovered parser/value provenance.  PDB field
    # names close the small set of real Constants fields absent from that
    # derivation without inventing values or XML provenance.
    constants: dict[int, dict[str, Any]] = {}
    for constant in load_json(RULES_CONSTANTS):
        for entry in constant["entries"]:
            constants[entry["offset"]] = {
                "name": constant["name"],
                "index": entry["index"],
                "of": constant["count"],
                "value": entry.get("stored"),
                "xml": entry.get("xml_value"),
            }
    pdb_constant_names = {
        field["offset"]: field["name"]
        for field in pdb_types["classes"]["Constants"]["fields"]
    }

    source_locations: dict[int, tuple[str, int | None]] = {}
    try:
        symbols = load_json(SYMBOLS)
        functions = symbols["functions"]
        values = functions.values() if isinstance(functions, dict) else functions
        for function in values:
            va = function.get("va") or function.get("address") or function.get("addr")
            if isinstance(va, str):
                va = int(va, 16)
            if va is None:
                continue
            source_file = (
                function.get("source_file")
                or function.get("src")
                or function.get("file")
            )
            source_line = function.get("line") or function.get("src_line")
            if source_file:
                source_locations[va] = (source_file, source_line)
    except (KeyError, TypeError, ValueError) as exc:
        print(f"warning: source map unavailable: {exc}", file=sys.stderr)

    disassembler = Cs(CS_ARCH_X86, CS_MODE_32)
    disassembler.detail = True

    def register_name(register: int) -> str | None:
        return disassembler.reg_name(register) if register else None

    def constant_reads(
        instructions: list[Any], lo: int, hi: int, seed_from: int
    ) -> tuple[list[dict[str, Any]], list[str]]:
        """Recover Constants[] reads while retaining register provenance.

        A register loaded from GameAccess::constants is a base only until it is
        overwritten or dereferenced.  The scratch generator failed to clear it
        after ``mov eax,[eax+off]``, causing later LeaderData/Setup reads to be
        mislabeled as Constants.  Memory-destination MOVs and alignment NOPs are
        not reads either.
        """

        bases: set[str] = set()
        reads: dict[int, str] = {}
        operations: list[str] = []

        for index in range(seed_from, hi):
            instruction = instructions[index]
            operands = instruction.operands

            if instruction.id == X86_INS_MOV and len(operands) == 2:
                destination, source = operands
                if (
                    source.type == X86_OP_MEM
                    and source.mem.base == 0
                    and source.mem.index == 0
                    and (source.mem.disp & 0xFFFFFFFF) in CONST_PTRS
                    and destination.type == X86_OP_REG
                ):
                    bases.add(normalize_register(register_name(destination.reg)))
                    continue
                if destination.type == X86_OP_REG and source.type == X86_OP_REG:
                    source_reg = normalize_register(register_name(source.reg))
                    destination_reg = normalize_register(register_name(destination.reg))
                    if source_reg in bases:
                        bases.add(destination_reg)
                    else:
                        bases.discard(destination_reg)
                    continue

            if index >= lo and instruction.id != X86_INS_NOP:
                for operand in operands:
                    if (
                        operand.type == X86_OP_MEM
                        and operand.mem.base
                        and operand.mem.index == 0
                        and operand.access & CS_AC_READ
                        and normalize_register(register_name(operand.mem.base)) in bases
                    ):
                        reads.setdefault(
                            operand.mem.disp & 0xFFFFFFFF,
                            f"{instruction.mnemonic} {instruction.op_str}",
                        )

            if index >= lo and instruction.id in ARITH:
                operations.append(ARITH[instruction.id])

            # Any write to a tracked destination destroys base provenance unless
            # one of the two MOV cases above already continued with an explicit
            # propagation decision.
            if operands and operands[0].type == X86_OP_REG and instruction.id not in (
                X86_INS_PUSH,
                X86_INS_CMP,
                X86_INS_TEST,
            ):
                bases.discard(normalize_register(register_name(operands[0].reg)))
            if instruction.id == X86_INS_CALL:
                for register in ("eax", "ecx", "edx"):
                    bases.discard(register)

        output = []
        for offset, text_ins in sorted(reads.items()):
            recovered = constants.get(offset)
            output.append(
                {
                    "offset": f"0x{offset:04x}",
                    "ins": text_ins,
                    "constant": (
                        recovered["name"]
                        if recovered
                        else pdb_constant_names.get(offset)
                    ),
                    "index": recovered["index"] if recovered else None,
                    "of": recovered["of"] if recovered else None,
                    "value": recovered["value"] if recovered else None,
                    "rules_xml": recovered["xml"] if recovered else None,
                }
            )
        return output, sorted(set(operations))

    def subject(predicate: str, argument: int | None) -> dict[str, Any] | None:
        if argument is None:
            return None
        if predicate == "has_wonder":
            return {
                "kind": "wonder",
                "type_index": argument,
                "name": type_names.get(argument),
            }
        if predicate == "has_tribe_bonus":
            return {
                "kind": "nation",
                "tribe_index": argument,
                "name": TRIBES[argument] if argument < 24 else None,
            }
        return {
            "kind": "rare",
            "type_index": argument,
            "name": type_names.get(argument),
        }

    sites: list[dict[str, Any]] = []
    for proc in procs:
        if not (text_lo <= proc["va"] < text_hi) or proc["va"] in PREDS:
            continue
        offset = proc["va"] - text_lo
        if offset + proc["size"] > len(text):
            continue
        instructions = list(
            disassembler.disasm(text[offset : offset + proc["size"]], proc["va"])
        )
        if not instructions:
            continue
        address_to_index = {
            instruction.address: index
            for index, instruction in enumerate(instructions)
        }
        source = source_locations.get(proc["va"])

        def emit(
            index: int,
            kind: str,
            predicate: str,
            argument: int | None,
            argument_confidence: str,
            lo: int,
            hi: int,
            polarity: str,
            note: str | None = None,
        ) -> None:
            constant_data, operations = constant_reads(
                instructions, lo, hi, max(0, index - 60)
            )
            sites.append(
                {
                    "site": f"0x{instructions[index].address:08x}",
                    "kind": kind,
                    "predicate": predicate,
                    "subject": subject(predicate, argument),
                    "subject_confidence": argument_confidence,
                    "polarity": polarity,
                    "function": proc["name"],
                    "function_va": f"0x{proc['va']:08x}",
                    "source_file": source[0] if source else None,
                    "source_line": source[1] if source else None,
                    "guarded_block": [
                        f"0x{instructions[lo].address:08x}",
                        f"0x{instructions[min(hi, len(instructions) - 1)].address:08x}",
                    ],
                    "arith": operations,
                    "constants": constant_data,
                    "note": note,
                }
            )

        for index, instruction in enumerate(instructions):
            # Explicit predicate call.
            if (
                instruction.id == X86_INS_CALL
                and instruction.operands
                and instruction.operands[0].type == X86_OP_IMM
                and (instruction.operands[0].imm & 0xFFFFFFFF) in PREDS
            ):
                predicate = PREDS[instruction.operands[0].imm & 0xFFFFFFFF]
                argument = None
                argument_source = None
                confidence = "literal"
                for prior in range(index - 1, max(-1, index - 45), -1):
                    candidate = instructions[prior]
                    if candidate.id == X86_INS_CALL:
                        break
                    if candidate.id == X86_INS_PUSH:
                        if candidate.operands[0].type == X86_OP_IMM:
                            argument = candidate.operands[0].imm & 0xFFFFFFFF
                        else:
                            argument_source = (
                                f"{candidate.mnemonic} {candidate.op_str}"
                            )
                        break

                if argument is None and argument_source is None:
                    for prior in range(index - 1, max(-1, index - 45), -1):
                        candidate = instructions[prior]
                        if (
                            candidate.id == X86_INS_PUSH
                            and candidate.operands[0].type == X86_OP_IMM
                        ):
                            value = candidate.operands[0].imm & 0xFFFFFFFF
                            if value in DOM[predicate]:
                                argument = value
                                confidence = "literal-across-call"
                                break

                if (
                    argument is None
                    and argument_source
                    and argument_source.startswith("push ")
                    and " ptr " not in argument_source
                ):
                    register = normalize_register(argument_source.split()[1])
                    push_index = None
                    for prior in range(index - 1, max(-1, index - 6), -1):
                        if instructions[prior].id == X86_INS_PUSH:
                            push_index = prior
                            break
                    if push_index is not None:
                        for prior in range(
                            push_index - 1, max(-1, push_index - 25), -1
                        ):
                            candidate = instructions[prior]
                            operands = candidate.operands
                            if (
                                not operands
                                or operands[0].type != X86_OP_REG
                                or normalize_register(
                                    register_name(operands[0].reg)
                                )
                                != register
                            ):
                                continue
                            if (
                                candidate.id == X86_INS_XOR
                                and len(operands) == 2
                                and operands[1].type == X86_OP_REG
                                and normalize_register(register_name(operands[1].reg))
                                == register
                            ):
                                argument, confidence = 0, "reg-zeroed"
                                break
                            if (
                                candidate.id == X86_INS_MOV
                                and operands[1].type == X86_OP_IMM
                            ):
                                argument = operands[1].imm & 0xFFFFFFFF
                                confidence = "reg-immediate"
                                break
                            if (
                                candidate.id == X86_INS_TEST
                                and len(operands) == 2
                                and operands[1].type == X86_OP_REG
                                and normalize_register(register_name(operands[1].reg))
                                == register
                                and prior + 1 < len(instructions)
                                and instructions[prior + 1].id == X86_INS_JNE
                            ):
                                argument, confidence = 0, "reg-zeroed"
                                break
                            if (
                                candidate.id == X86_INS_CMP
                                and operands[1].type == X86_OP_IMM
                                and prior + 1 < len(instructions)
                                and instructions[prior + 1].id
                                in (X86_INS_JNE, X86_INS_JE)
                            ):
                                argument = operands[1].imm & 0xFFFFFFFF
                                confidence = "reg-guarded"
                                break
                            break

                if argument is None:
                    confidence = "dynamic"
                polarity = "unknown"
                lo = index + 1
                hi = min(len(instructions), index + 30)
                for following in range(index + 1, min(len(instructions), index + 6)):
                    candidate = instructions[following]
                    if candidate.id in (X86_INS_TEST, X86_INS_CMP):
                        continue
                    if candidate.id in (X86_INS_JE, X86_INS_JNE):
                        target = candidate.operands[0].imm & 0xFFFFFFFF
                        polarity = (
                            "if_true" if candidate.id == X86_INS_JE else "if_false"
                        )
                        lo = following + 1
                        hi = address_to_index.get(
                            target, min(len(instructions), following + 60)
                        )
                        if hi <= lo:
                            hi = min(len(instructions), lo + 40)
                        hi = min(hi, lo + 120)
                        break
                    if candidate.id == X86_INS_CALL:
                        break
                note = None
                if argument is None:
                    note = DYNAMIC_NOTES.get(
                        instruction.address,
                        f"argument is dynamic: {argument_source}",
                    )
                emit(
                    index,
                    "call",
                    predicate,
                    argument,
                    confidence,
                    lo,
                    hi,
                    polarity,
                    note,
                )
                continue

            # Compiler-inlined has_rare: the rare_conquest half of the pair of
            # BitMask<44> byte tests.  The rare half occurs a few bytes earlier.
            if (
                instruction.id == X86_INS_TEST
                and len(instruction.operands) == 2
                and instruction.operands[0].type == X86_OP_MEM
                and instruction.operands[1].type == X86_OP_IMM
                and instruction.operands[0].size == 1
            ):
                displacement = instruction.operands[0].mem.disp & 0xFFFFFFFF
                relative = (
                    displacement - LEADERS if displacement >= LEADERS else displacement
                )
                if not (RAREC_PAY <= relative < RAREC_PAY + 6):
                    continue
                immediate = instruction.operands[1].imm & 0xFF
                if immediate == 0 or immediate & (immediate - 1):
                    continue
                if (
                    index + 1 >= len(instructions)
                    or instructions[index + 1].id not in (X86_INS_JE, X86_INS_JNE)
                ):
                    continue
                bit = (relative - RAREC_PAY) * 8 + immediate.bit_length() - 1
                if bit >= 44:
                    continue
                branch = instructions[index + 1]
                target = branch.operands[0].imm & 0xFFFFFFFF
                polarity = "if_true" if branch.id == X86_INS_JE else "if_false"
                lo = index + 2
                hi = address_to_index.get(target, min(len(instructions), lo + 60))
                if hi <= lo:
                    hi = min(len(instructions), lo + 40)
                hi = min(hi, lo + 120)
                emit(
                    index,
                    "inline",
                    "has_rare",
                    6 + bit,
                    "bitmask",
                    lo,
                    hi,
                    polarity,
                    "has_rare inlined as `rare[bit] || rare_conquest[bit]`; this "
                    "is the rare_conquest half, the rare half is a few bytes earlier",
                )

    metadata: dict[str, Any] = {
        "generated_from": (
            "ron-bin/riseofnations.exe (sha256 30478a44…625079), PE32 i386, "
            "image base 0x00400000"
        ),
        "method": (
            "capstone linear disassembly of every PDB-known procedure; call sites of "
            "the four predicates and inlined has_rare BitMask tests; Constants[] reads "
            "recovered by tracking registers loaded from GameAccess::constants "
            "[0x00C061F0] / GameAccessConst::constantsc [0x00C061E4]"
        ),
        "provenance": "[measured] structure. No oracle run: no Tier A or B claim.",
        "predicates": {
            "has_wonder": {
                "va": "0x006ebc10",
                "semantics": (
                    "returns bit0 if the leader owns a completed building of that "
                    "wonder type assigned to a city (BuildData::city >= 0), except "
                    "REDFORT(534) which is exempt from the city test; returns bit1 if "
                    "LeaderData::conquest_wonders (BitMask<17> payload +0x6D74) has "
                    "the bit AND epoch[Civic] >= ctw_wonder_min_civics(4). Callers "
                    "test != 0 unless noted."
                ),
                "domain": "TypeIndex 526..542 (BASE_WONDERTYPES..END_WONDERTYPES-1)",
            },
            "has_tribe_bonus": {
                "va": "0x006e1370",
                "semantics": (
                    "false if game option [0x00C061E8+0x80] & 4, or the leader has "
                    "no tribe; true if LeaderData::conquest_racial_powers "
                    "(BitMask<24> payload +0x6D94) has the bit; false if LeaderData "
                    "flags byte +4 & 0x40; otherwise tribes[leader.tribe].bonus_id "
                    "(+0x54, stride 0x5F0) == arg."
                ),
                "domain": "tribe index 0..23, in ron-data/rules.xml <TRIBES> order",
            },
            "has_rare": {
                "va": "0x006e0770",
                "semantics": (
                    "rare[t-6] || rare_conquest[t-6]  (BitMask<44> payloads "
                    "+0x6DA4 / +0x6DCC)"
                ),
                "domain": "TypeIndex 6..49 (BASE_RARE .. BASE_RARE+NUM_RARES-1)",
            },
            "has_rare_conquest": {
                "va": "0x006d9320",
                "semantics": "t < 6 || rare_conquest[t-6]",
                "domain": "TypeIndex 0..49",
            },
        },
        "leaderdata_bitmasks": {
            "conquest_wonders": "BitMask<17> @ +0x6D68, payload +0x6D74",
            "conquest_wonders_in_game": "BitMask<17> @ +0x6D78, payload +0x6D84",
            "conquest_racial_powers": "BitMask<24> @ +0x6D88, payload +0x6D94",
            "rare": "BitMask<44> @ +0x6D98, payload +0x6DA4",
            "rare_owned": "BitMask<44> @ +0x6DAC, payload +0x6DB8",
            "rare_conquest": "BitMask<44> @ +0x6DC0, payload +0x6DCC",
        },
        "wonders": {str(index): type_names.get(index) for index in range(526, 543)},
        "nations": {str(index): name for index, name in enumerate(TRIBES)},
        "rares": {str(index): type_names.get(index) for index in range(6, 50)},
    }

    sites.sort(key=lambda site: site["site"])
    metadata["counts"] = {
        "total": len(sites),
        "by_predicate": {
            predicate: sum(1 for site in sites if site["predicate"] == predicate)
            for predicate in sorted({site["predicate"] for site in sites})
        },
        "by_kind": {
            kind: sum(1 for site in sites if site["kind"] == kind)
            for kind in ("call", "inline")
        },
        "with_constant": sum(1 for site in sites if site["constants"]),
        "subject_unresolved": sum(1 for site in sites if site["subject"] is None),
    }

    for key, expected in EXPECTED_COUNTS.items():
        if metadata["counts"][key] != expected:
            raise SystemExit(
                f"effect-site census drifted for {key}: "
                f"expected {expected!r}, got {metadata['counts'][key]!r}"
            )

    return {"_meta": metadata, "sites": sites}


def parse_args() -> argparse.Namespace:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument(
        "output",
        nargs="?",
        type=Path,
        default=DEFAULT_OUTPUT,
        help=f"output ledger (default: {DEFAULT_OUTPUT.relative_to(ROOT)})",
    )
    parser.add_argument(
        "--check",
        action="store_true",
        help="compare regenerated bytes with OUTPUT instead of writing",
    )
    return parser.parse_args()


def main() -> int:
    args = parse_args()
    output = args.output.resolve()
    ledger = build_ledger()
    rendered = json.dumps(ledger, indent=1)
    counts = ledger["_meta"]["counts"]

    if args.check:
        try:
            current = output.read_text(encoding="utf-8")
        except FileNotFoundError:
            print(f"effects ledger is missing: {output}", file=sys.stderr)
            return 1
        if current == rendered:
            print(
                f"effects ledger is current: {counts['total']} sites "
                f"({counts['by_kind']['call']} call, "
                f"{counts['by_kind']['inline']} inline)"
            )
            return 0

        print(f"effects ledger is stale: {output}", file=sys.stderr)
        diff = list(
            difflib.unified_diff(
                current.splitlines(),
                rendered.splitlines(),
                fromfile=str(output),
                tofile="regenerated",
                lineterm="",
            )
        )
        limit = 200
        for line in diff[:limit]:
            print(line, file=sys.stderr)
        if len(diff) > limit:
            print(f"... {len(diff) - limit} more diff lines", file=sys.stderr)
        return 1

    output.parent.mkdir(parents=True, exist_ok=True)
    output.write_text(rendered, encoding="utf-8")
    print(json.dumps(counts, indent=1))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
