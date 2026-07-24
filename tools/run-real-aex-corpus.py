#!/usr/bin/env python3
"""Execute Issue #9 by producing and validating one Issue #4 bundle per cell.

Third-party bytes exist only inside an ephemeral secure execution tree. The
persisted triage and gap documents contain identities/results, never AEX bytes
or machine-local paths.
"""
from __future__ import annotations

import argparse, hashlib, importlib.util, json, math, os, re, shutil, sys, tempfile
from collections import defaultdict
from contextlib import contextmanager
from itertools import product
from pathlib import Path
from typing import Any
from jsonschema import Draft202012Validator

ROOT = Path(__file__).resolve().parents[1]; SCHEMAS = ROOT / "schemas"
if str(ROOT / "tools") not in sys.path:
    sys.path.insert(0, str(ROOT / "tools"))

def _load(name: str, path: Path):
    spec = importlib.util.spec_from_file_location(name, path); module = importlib.util.module_from_spec(spec); spec.loader.exec_module(module); return module

BUNDLE = _load("aexcompat_issue4_bundle", ROOT / "tools/run-conformance-bundle.py")
VALIDATOR = _load("aexcompat_issue4_validator", ROOT / "tools/conformance_bundle_validator.py")
KNOWN_SUITES={"PF World Suite","PF Iterate8 Suite","PF iterate16 Suite","PF iterateFloat Suite","PF Pixel Data Suite","PF World Transform Suite","PF Fill Matte Suite","PF Path Data Suite","PF Path Query Suite","PF Sampling8 Suite","PF Sampling16 Suite","PF SamplingFloat Suite","PF Param Utils Suite","PF Effect UI Suite","PF AE Channel Suite","AEGP Item Suite","AEGP Comp Suite","AEGP Layer Suite","AEGP Effect Suite","AEGP Stream Suite","AEGP Render Suite"}

def strict_json(path: Path) -> Any:
    def unique(pairs):
        result = {}
        for key, value in pairs:
            if key in result: raise ValueError(f"duplicate JSON key: {key}")
            result[key] = value
        return result
    value = json.loads(path.read_text(encoding="utf-8"), object_pairs_hook=unique,
                       parse_constant=lambda value: (_ for _ in ()).throw(ValueError(f"non-finite JSON: {value}")))
    # parse_constant only catches the Infinity/NaN literals; an overflowing number
    # such as 1e999 decodes to float('inf') without it, so reject non-finite floats
    # recursively before they reach canonical()/the Issue #4 manifest (mirrors the
    # Issue #4 loader's strict_json_loads).
    def reject_overflowed(item):
        if isinstance(item, float) and not math.isfinite(item): raise ValueError(f"non-finite JSON number: {item}")
        if isinstance(item, dict):
            for child in item.values(): reject_overflowed(child)
        elif isinstance(item, list):
            for child in item: reject_overflowed(child)
    reject_overflowed(value); return value

def load_validated(path: Path, schema: str) -> Any:
    value = strict_json(path); Draft202012Validator(strict_json(SCHEMAS / schema)).validate(value); return value

def canonical(value: Any) -> bytes:
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, separators=(",", ":")) + "\n").encode()

def atomic_write(path: Path, value: Any):
    reject_private_paths(value)
    temporary = path.with_name(path.name + ".tmp")
    temporary.write_bytes(canonical(value)); temporary.replace(path)

def reject_private_paths(value: Any):
    if isinstance(value, dict):
        for item in value.values(): reject_private_paths(item)
    elif isinstance(value, list):
        for item in value: reject_private_paths(item)
    elif isinstance(value, str):
        folded=value.casefold()
        normalized=value.replace("\\","/")
        if re.match(r"^[a-z]:[\\/]",value,re.IGNORECASE) or value.startswith(("/","\\")) or folded.startswith(("\\\\?\\","\\\\.\\","\\??\\","\\device\\")) or normalized==".." or normalized.startswith("../") or "/../" in normalized or normalized.endswith("/.."):
            raise ValueError("public evidence contains an absolute or device path")

def output_destinations(public: Path, private: Path):
    public=public.resolve();private=private.resolve();left=os.path.normcase(str(public));right=os.path.normcase(str(private));separator=os.sep
    if left==right or left.startswith(right+separator) or right.startswith(left+separator): raise ValueError("public and private outputs must be distinct non-nested paths")
    if public.exists() or private.exists(): raise ValueError("output paths must not already exist")
    return public,private

def publish_outputs(public_tmp: Path, private_tmp: Path, public: Path, private: Path):
    private_tmp.replace(private)
    try: public_tmp.replace(public)
    except BaseException:
        shutil.rmtree(private,ignore_errors=True)
        raise

def artifact(path: Path, root: Path) -> dict[str, Any]: return BUNDLE.artifact_for(path, root)

def validate_inventory(inventory):
    entries = inventory["entries"]
    if len({x["id"] for x in entries}) != len(entries) or len({x["sha256"] for x in entries}) != len(entries): raise ValueError("corpus ids and SHA identities must be unique")
    if len({x["supplier"] for x in entries}) < 3: raise ValueError("at least three suppliers are required")
    if any(not x["local_only"] for x in entries): raise ValueError("AEX bytes must remain local-only")

def bind_locator(inventory, locator):
    public = {x["id"]: x for x in inventory["entries"]}
    if set(public) != set(locator["plugins"]): raise ValueError("locator ids must exactly match inventory")
    bound = {}
    for plugin_id, item in locator["plugins"].items():
        root = BUNDLE._real_directory(Path(item["source_root"]))
        aex = item["aex"]
        if (aex["sha256"], aex["size_bytes"]) != (public[plugin_id]["sha256"], public[plugin_id]["size_bytes"]): raise ValueError(f"public/local identity mismatch: {plugin_id}")
        names = [Path(aex["path"]).name, *[Path(x["path"]).name for x in item.get("dependencies", [])]]
        if len({x.casefold() for x in names}) != len(names): raise ValueError(f"dependency basename collision: {plugin_id}")
        bound[plugin_id] = (root, aex, item.get("dependencies", []))
    return bound

def matrix_cases(inventory, matrix):
    for ordinal, values in enumerate(product(inventory["entries"], matrix["render_paths"], matrix["depths"], matrix["times"], range(len(matrix["parameter_sets"]))), 1):
        entry, path, depth, timing, parameter_index = values
        yield {"case_id":f"case-{ordinal:06d}", "plugin_id":entry["id"], "plugin_sha256":entry["sha256"], "render_path":path, "depth":depth, "time":timing, "parameter_set_index":parameter_index}

@contextmanager
def argv_for(manifest: Path, output: Path):
    previous = sys.argv; sys.argv = [str(ROOT / "tools/run-conformance-bundle.py"), "--manifest", str(manifest), "--out", str(output), "--allow-failures"]
    try: yield
    finally: sys.argv = previous

def copy_external(source: Path, destination: Path) -> dict[str, Any]:
    source_root = BUNDLE._real_directory(source.parent); identity = artifact(source, source_root)
    BUNDLE.copy_verified_artifact(source_root, identity, destination)
    return {"path": destination.name, "sha256": identity["sha256"], "size_bytes": identity["size_bytes"]}

@contextmanager
def pinned_common_sources(runner: Path, input_path: Path):
    previous_root = BUNDLE.ROOT
    with tempfile.TemporaryDirectory(prefix="aexcompat-corpus-pinned-") as temporary:
        pinned = Path(temporary) / "repository"; pinned.mkdir()
        pinned_runner = pinned / "common/runner" / runner.name; copy_external(runner, pinned_runner)
        pinned_input = pinned / "common/input" / input_path.name; copy_external(input_path, pinned_input)
        for relative in BUNDLE.NATIVE_WORKERS:
            copy_external(previous_root / Path(*relative.split("/")), pinned / Path(*relative.split("/")))
        BUNDLE.ROOT = pinned
        try: yield pinned_runner, pinned_input
        finally: BUNDLE.ROOT = previous_root

def common_identities(identities):
    return {role: identities[role] for role in ("input","runner","workers")}

def sanitized_identities(identities):
    def clean(role, item): return {"role":role,"sha256":item["sha256"],"size_bytes":item["size_bytes"]}
    return {"aex":clean("aex",identities["aex"]),"dependencies":[clean(f"dependency-{index:03d}",item) for index,item in enumerate(identities["dependencies"])],"input":clean("input",identities["input"]),"runner":clean("runner",identities["runner"]),"workers":[clean(f"worker-{index:03d}",item) for index,item in enumerate(identities["workers"])]}

def execute_case(case, matrix, local, runner: Path, input_path: Path):
    source_root, aex, dependencies = local
    with tempfile.TemporaryDirectory(prefix="aexcompat-corpus-cell-") as temporary:
        stage = Path(temporary) / "fixture"; stage.mkdir()
        plugin_dest = stage / "plugin" / Path(aex["path"]).name
        BUNDLE.copy_verified_artifact(source_root, aex, plugin_dest)
        plugin_identity = {"path":plugin_dest.relative_to(stage).as_posix(), "sha256":aex["sha256"], "size_bytes":aex["size_bytes"]}
        dependency_identities = []
        for item in dependencies:
            destination = stage / "dependencies" / Path(item["path"]).name
            BUNDLE.copy_verified_artifact(source_root, item, destination)
            dependency_identities.append({"path":destination.relative_to(stage).as_posix(), "sha256":item["sha256"], "size_bytes":item["size_bytes"]})
        runner_dest = stage / "runner" / runner.name; runner_identity = copy_external(runner, runner_dest); runner_identity["path"] = runner_dest.relative_to(stage).as_posix()
        input_dest = stage / "inputs" / input_path.name; input_identity = copy_external(input_path, input_dest); input_identity["path"] = input_dest.relative_to(stage).as_posix()
        manifest = {"schema_version":1, "fixture_id":case["case_id"], "plugin":{"aex":plugin_identity,"dependencies":dependency_identities}, "input":input_identity, "runner":runner_identity, "requested_depths":[case["depth"]], "execution":{"render_path":case["render_path"],"time":case["time"],"parameters":matrix["parameter_sets"][case["parameter_set_index"]],"premultiplication":"straight","color_management":{"enabled":False,"working_space":None},"linear_light":False,"renderer":"AEXCompat CPU"}, "oracle":{"state":"not_captured","identity_match":False}}
        manifest_path = stage / "manifest.json"; manifest_path.write_bytes(canonical(manifest)); bundle = Path(temporary) / "bundle"
        with argv_for(manifest_path, bundle):
            if BUNDLE.main() != 0: raise RuntimeError("Issue #4 bundle runner failed")
        report_path = bundle / "report.json"; report_bytes = report_path.read_bytes(); report = strict_json(report_path); VALIDATOR.validate_bundle(manifest, report, bundle)
        result = report["results"][0]
        if result["depth"] != case["depth"] or result["selector"]["render_path"] != case["render_path"]: raise ValueError("validated report does not match requested matrix cell")
        evidence={"manifest.json":manifest_path.read_bytes(),"report.json":report_bytes,"run.json":(bundle/"diagnostics/run.json").read_bytes()}
        return report, result, hashlib.sha256(report_bytes).hexdigest(), evidence

def retain_private_evidence(root: Path, case_id: str, evidence):
    destination=root/"evidence"/case_id;destination.mkdir(parents=True)
    for name,data in evidence.items():
        (destination/name).write_bytes(data)

def normalize_classification(value):
    # empty_result is a legal empty SmartFX render (Issue #4 schema/runner), not a
    # capability gap, so map it to the non-gap "ok" bucket rather than KeyError-ing
    # and aborting the whole matrix run.
    return {"ok":"ok","empty_result":"ok","loader_error":"loader_error","unsupported":"selector_error","selector_error":"selector_error","missing_suite":"missing_suite","crashed":"crashed","timeout_killed":"timeout","host_validation_error":"host_validation_error","invalid_output":"host_validation_error","nonzero_exit":"host_validation_error"}[value]

def suite_id(name: str, version: int) -> str: return "suite-" + hashlib.sha256(f"{name}\0{version}".encode()).hexdigest()[:20]

def aggregate(results):
    identities = defaultdict(set)
    for result in results:
        if result["classification"] != "missing_suite": continue
        for item in result["missing_suites"]: identities[(item["name"],item["version"])].add(result["plugin_sha256"])
    return [{"suite_id":suite_id(name,version),"version":version,"distinct_aex_sha_count":len(shas)} for (name,version),shas in sorted(identities.items())]

def gap_records(rows,matrix,public_root):
    pending=[]
    stage={"loader_error":"loader","selector_error":"selector","missing_suite":"suite","host_validation_error":"host_validation","crashed":"process","timeout":"process"}
    for row in rows:
        if row["classification"]=="ok": continue
        suites=[]
        for item in row["missing_suites"]:
            value={"suite_id":suite_id(item["name"],item["version"]),"version":item["version"]}
            if item["name"] in KNOWN_SUITES: value["canonical_name"]=item["name"]
            suites.append(value)
        parameters=[{"index":item["index"],"type":item["type"]} for item in matrix["parameter_sets"][row["parameter_set_index"]]]
        pending.append(({"render_path":row["render_path"],"depth":row["depth"],"time":row["time"],"parameter_shape":parameters,"classification":row["classification"],"stage":stage[row["classification"]],"error_code":row["selector_error_code"],"missing_suites":suites,"host_identities":{"input":row["identities"]["input"],"runner":row["identities"]["runner"],"workers":row["identities"]["workers"]}},row["case_id"]))
    grouped=defaultdict(list)
    for value,case_id in pending: grouped[canonical(value)].append((value,case_id))
    records=[];mapping=[]
    replay=["python","tools/run-real-aex-corpus.py","--inventory","<PUBLIC_INVENTORY>","--locator","<PRIVATE_LOCATOR>","--matrix","<MATRIX>","--runner","<PINNED_RUNNER>","--input","<INPUT>","--out","<NEW_PUBLIC_OUT>","--private-evidence-out","<NEW_PRIVATE_OUT>","--case-list","<PRIVATE_CASE_LIST>"]
    evidence_schema=Draft202012Validator(strict_json(SCHEMAS/"real-aex-public-evidence.schema.json"))
    for index,key in enumerate(sorted(grouped),1):
        values=grouped[key];value=values[0][0];gap_id=f"gap-{index:06d}"; evidence={"schema_version":1,"gap_id":gap_id,"occurrence_count":len(values),**value};evidence_schema.validate(evidence);reject_private_paths(evidence);destination=public_root/"evidence"/f"{gap_id}.json";destination.parent.mkdir(parents=True,exist_ok=True);data=canonical(evidence);destination.write_bytes(data)
        records.append({"gap_id":gap_id,"occurrence_count":len(values),**{field:value[field] for field in ("render_path","depth","time","parameter_shape","classification","stage","error_code","missing_suites")},"evidence":{"path":destination.relative_to(public_root).as_posix(),"sha256":hashlib.sha256(data).hexdigest()},"replay":replay})
        mapping.append({"gap_id":gap_id,"case_ids":sorted(case_id for _,case_id in values)})
    return records,mapping

def selected_case_ids(case_id,case_list):
    if case_id is not None: return {case_id}
    if case_list is None: return None
    values=strict_json(case_list)
    if not isinstance(values,list) or not values: raise ValueError("--case-list must be a non-empty JSON array")
    if any(not isinstance(value,str) or re.fullmatch(r"case-[0-9]{6}",value) is None for value in values): raise ValueError("--case-list contains an invalid case id")
    if len(set(values))!=len(values): raise ValueError("--case-list contains duplicate case ids")
    return set(values)

def main():
    parser=argparse.ArgumentParser(); parser.add_argument("--inventory",type=Path,required=True);parser.add_argument("--locator",type=Path,required=True);parser.add_argument("--matrix",type=Path,required=True);parser.add_argument("--runner",type=Path,required=True);parser.add_argument("--input",type=Path,required=True);parser.add_argument("--out",type=Path,required=True);parser.add_argument("--private-evidence-out",type=Path,required=True);selection=parser.add_mutually_exclusive_group();selection.add_argument("--case-id");selection.add_argument("--case-list",type=Path);args=parser.parse_args()
    inventory=load_validated(args.inventory,"real-aex-corpus.schema.json");matrix=load_validated(args.matrix,"real-aex-matrix.schema.json");locator=load_validated(args.locator,"real-aex-locator.schema.json");validate_inventory(inventory);bound=bind_locator(inventory,locator)
    public_destination,private_destination=output_destinations(args.out,args.private_evidence_out)
    public_destination.parent.mkdir(parents=True,exist_ok=True);private_destination.parent.mkdir(parents=True,exist_ok=True);publish=Path(tempfile.mkdtemp(prefix=".aexcompat-corpus-publish-",dir=public_destination.parent));private_publish=Path(tempfile.mkdtemp(prefix=".aexcompat-corpus-private-",dir=private_destination.parent))
    rows=[]; expected_common=None;requested_cases=selected_case_ids(args.case_id,args.case_list);matched_cases=set()
    try:
     with pinned_common_sources(args.runner.resolve(strict=True),args.input.resolve(strict=True)) as (pinned_runner,pinned_input):
      for case in matrix_cases(inventory,matrix):
        if requested_cases is not None and case["case_id"] not in requested_cases: continue
        matched_cases.add(case["case_id"])
        report,result,report_hash,evidence=execute_case(case,matrix,bound[case["plugin_id"]],pinned_runner,pinned_input)
        if hashlib.sha256(evidence["report.json"]).hexdigest()!=report_hash: raise ValueError("private report evidence hash mismatch")
        observed=common_identities(report["identities"])
        if expected_common is None: expected_common=observed
        elif observed != expected_common: raise ValueError("run-wide runner, input, or worker identity changed")
        retain_private_evidence(private_publish,case["case_id"],evidence)
        rows.append({**case,"classification":normalize_classification(result["classification"]),"selector_error_code":result["selector"]["error_code"],"missing_suites":result.get("missing_suites",[]),"private_report_path":f"evidence/{case['case_id']}/report.json","private_report_sha256":report_hash,"identities":sanitized_identities(report["identities"])})
     if requested_cases is not None and matched_cases!=requested_cases: raise ValueError("case selection contains an unknown matrix cell")
     if not rows: raise ValueError("case selection did not select a matrix cell")
     suites=aggregate(rows)
     inventory_hash=hashlib.sha256(canonical(inventory)).hexdigest();matrix_hash=hashlib.sha256(canonical(matrix)).hexdigest()
     triage={"schema_version":1,"inventory_sha256":inventory_hash,"matrix_sha256":matrix_hash,"results":rows,"missing_suite_aggregation":suites};Draft202012Validator(strict_json(SCHEMAS/"real-aex-triage.schema.json")).validate(triage);atomic_write(private_publish/"triage.json",triage)
     records,mapping=gap_records(rows,matrix,publish);atomic_write(private_publish/"gap-map.json",{"schema_version":1,"mapping":mapping})
     gaps={"schema_version":1,"corpus_shape":{"aex_count":len(inventory["entries"]),"supplier_count":len({item["supplier"] for item in inventory["entries"]})},"matrix_shape":{"render_path_count":len(matrix["render_paths"]),"depth_count":len(matrix["depths"]),"time_count":len(matrix["times"]),"parameter_shape_count":len(matrix["parameter_sets"])},"records":records,"missing_suite_aggregation":suites};Draft202012Validator(strict_json(SCHEMAS/"real-aex-gaps.schema.json")).validate(gaps);atomic_write(publish/"reproducible-gaps.json",gaps)
     publish_outputs(publish,private_publish,public_destination,private_destination);publish=None;private_publish=None;return 0
    finally:
     if publish is not None: shutil.rmtree(publish,ignore_errors=True)
     if private_publish is not None: shutil.rmtree(private_publish,ignore_errors=True)

if __name__=="__main__": raise SystemExit(main())
