Exit code: 0
Wall time: 0.3 seconds
Output:
import importlib.util, json
from pathlib import Path
import pytest
from jsonschema import Draft202012Validator

ROOT=Path(__file__).resolve().parents[1]
def load(name,path):
    spec=importlib.util.spec_from_file_location(name,path);m=importlib.util.module_from_spec(spec);spec.loader.exec_module(m);return m
corpus=load("corpus",ROOT/"tools/run-real-aex-corpus.py")
bundle=load("bundle_test",ROOT/"tools/run-conformance-bundle.py")

def test_public_inventory_matrix_and_case_count():
    inventory=corpus.load_validated(ROOT/"corpus/real-aex-public.json","real-aex-corpus.schema.json");matrix=corpus.load_validated(ROOT/"corpus/common-matrix.json","real-aex-matrix.schema.json");corpus.validate_inventory(inventory)
    assert len(inventory["entries"])==5 and len({x["supplier"] for x in inventory["entries"]})==3
    assert set(matrix["render_paths"])=={"classic","smartfx"} and len(list(corpus.matrix_cases(inventory,matrix)))==120
    assert "Program Files" not in (ROOT/"corpus/real-aex-public.json").read_text(encoding="utf-8")

def test_all_corpus_schemas_are_valid_draft_2020_12():
    for name in ("real-aex-corpus.schema.json","real-aex-locator.schema.json","real-aex-matrix.schema.json","real-aex-triage.schema.json","real-aex-gaps.schema.json","real-aex-public-evidence.schema.json"):
        Draft202012Validator.check_schema(json.loads((ROOT/"schemas"/name).read_text(encoding="utf-8")))

def test_suite_aggregation_is_distinct_sha_and_gap_safe():
    rows=[{"classification":"missing_suite","plugin_sha256":"a"*64,"missing_suites":[{"name":"Private Looking Suite","version":2}]},{"classification":"missing_suite","plugin_sha256":"a"*64,"missing_suites":[{"name":"Private Looking Suite","version":2}]},{"classification":"missing_suite","plugin_sha256":"b"*64,"missing_suites":[{"name":"Private Looking Suite","version":2}]}]
    value=corpus.aggregate(rows);encoded=json.dumps(value)
    assert value[0]["distinct_aex_sha_count"]==2 and "Private Looking" not in encoded and value[0]["suite_id"].startswith("suite-")

def test_issue4_runner_maps_every_explicit_path_and_depth():
    assert set(bundle.DEPTH_COMMANDS)=={(path,depth) for path in ("classic","smartfx") for depth in ("argb8","argb16","argb32f")}
    assert bundle.DEPTH_COMMANDS[("classic","argb32f")]=="--render-experimental-request-32"

def test_real_corpus_aggregation_explicitly_allows_bundle_failures():
    source = (ROOT / "tools" / "run-real-aex-corpus.py").read_text(encoding="utf-8")
    assert "--allow-failures" in source

def test_normalize_classification_covers_every_report_schema_class():
    schema=json.loads((ROOT/"schemas/conformance-report.schema.json").read_text(encoding="utf-8"))
    classes=schema["$defs"]["depth_result"]["properties"]["classification"]["enum"]
    for value in classes: corpus.normalize_classification(value)  # no KeyError for any schema class
    assert corpus.normalize_classification("empty_result")=="ok"  # a legal empty SmartFX render is not a gap

def test_classic_success_and_invalid_output_never_become_smartfx(tmp_path):
    world={"width":1,"height":1,"row_bytes":4,"pixel_format":"argb8","premultiplication":"straight","extent_hint":{"left":0,"top":0,"right":1,"bottom":1}}
    failed=bundle.normalize_harness_report("argb8",{},tmp_path/"absent",world,"straight","classic")
    assert failed["selector"]["render_path"]=="classic"
    output=tmp_path/"out";output.write_bytes(b"x")
    value={"passed":True,"input_world":world,"output_world":world,"render_path":"classic"}
    assert bundle.normalize_harness_report("argb8",value,output,world,"straight","classic")["selector"]["render_path"]=="classic"
    value["input_world"] = []
    malformed=bundle.normalize_harness_report("argb8",value,output,world,"straight","classic")
    assert malformed["classification"]=="invalid_output" and malformed["selector"]["render_path"]=="classic"

def test_loader_error_precedes_other_structured_hints():
    world={"width":1,"height":1,"row_bytes":4,"pixel_format":"argb8","premultiplication":"straight","extent_hint":{"left":0,"top":0,"right":1,"bottom":1}}
    result=bundle.normalize_structured_failure("argb8",{"plugin_kind":"aegp_candidate","classification":"missing_suite","missing_suites":[{"name":"PF World Suite","version":2}]},world,"classic")
    assert result["classification"]=="loader_error" and result["selector"]["render_path"]=="classic" and "missing_suites" not in result

def test_locator_requires_public_identity_and_exact_dependency_basenames(tmp_path):
    root=tmp_path/"source";root.mkdir();(root/"effect.aex").write_bytes(b"a");(root/"KeepName.dll").write_bytes(b"d")
    aex={"path":"effect.aex","sha256":bundle.sha256(root/"effect.aex"),"size_bytes":1};dep={"path":"KeepName.dll","sha256":bundle.sha256(root/"KeepName.dll"),"size_bytes":1}
    inventory={"entries":[{"id":"x","sha256":aex["sha256"],"size_bytes":1}]};bound=corpus.bind_locator(inventory,{"plugins":{"x":{"source_root":str(root),"aex":aex,"dependencies":[dep]}}})
    assert Path(bound["x"][2][0]["path"]).name=="KeepName.dll"

def test_strict_json_rejects_duplicates_and_nonfinite(tmp_path):
    p=tmp_path/"x.json";p.write_text('{"x":1,"x":2}')
    with pytest.raises(ValueError,match="duplicate"):corpus.strict_json(p)
    p.write_text('{"x":NaN}')
    with pytest.raises(ValueError,match="non-finite"):corpus.strict_json(p)
    p.write_text('{"x":1e999}')  # overflows to inf without hitting parse_constant
    with pytest.raises(ValueError,match="non-finite"):corpus.strict_json(p)
    p.write_text('{"a":[{"b":-1e999}]}')  # nested overflow is rejected recursively
    with pytest.raises(ValueError,match="non-finite"):corpus.strict_json(p)

@pytest.mark.parametrize("value",[r"C:\private\x",r"\\server\share",r"\\?\C:\x",r"\\.\pipe\x",r"\??\C:\x",r"\Device\HarddiskVolume1\x",r"\private\x","/home/private/x","../private/x",r"safe\..\private"])
def test_public_evidence_rejects_absolute_unc_and_device_paths(value):
    with pytest.raises(ValueError,match="absolute or device"):corpus.reject_private_paths({"nested":[value]})

def test_output_destinations_reject_same_and_nested_paths(tmp_path):
    with pytest.raises(ValueError,match="non-nested"):corpus.output_destinations(tmp_path/"same",tmp_path/"same")
    with pytest.raises(ValueError,match="non-nested"):corpus.output_destinations(tmp_path/"outer",tmp_path/"outer/private")

def test_private_case_list_replays_every_group_occurrence(tmp_path):
    case_list=tmp_path/"gap-cases.json";case_list.write_text('["case-000001","case-000025","case-000049","case-000073"]')
    assert corpus.selected_case_ids(None,case_list)=={"case-000001","case-000025","case-000049","case-000073"}
    for value in ('[]','["case-000001","case-000001"]','["private-plugin"]'):
        case_list.write_text(value)
        with pytest.raises(ValueError): corpus.selected_case_ids(None,case_list)

def test_public_group_replay_uses_complete_private_case_list(tmp_path):
    identities={"input":{"role":"input","sha256":"1"*64,"size_bytes":1},"runner":{"role":"runner","sha256":"2"*64,"size_bytes":1},"workers":[{"role":f"worker-{index:03d}","sha256":str(index+3)*64,"size_bytes":1} for index in range(3)]}
    rows=[{"case_id":case_id,"classification":"loader_error","render_path":"classic","depth":"argb8","time":{"value":0,"scale":1},"parameter_set_index":0,"selector_error_code":None,"missing_suites":[],"identities":identities} for case_id in ("case-000001","case-000025","case-000049","case-000073")]
    records,mapping=corpus.gap_records(rows,{"parameter_sets":[[]]},tmp_path)
    assert records[0]["occurrence_count"]==4
    assert records[0]["replay"][-2:]==["--case-list","<PRIVATE_CASE_LIST>"]
    assert mapping[0]["case_ids"]==["case-000001","case-000025","case-000049","case-000073"]

def test_publish_rolls_back_private_when_public_commit_fails(tmp_path,monkeypatch):
    public_tmp=tmp_path/"public-tmp";private_tmp=tmp_path/"private-tmp";public_tmp.mkdir();private_tmp.mkdir();public=tmp_path/"public";private=tmp_path/"private";original=Path.replace;calls=0
    def replace(path,target):
        nonlocal calls;calls+=1
        if calls==2:raise OSError("injected public publish failure")
        return original(path,target)
    monkeypatch.setattr(Path,"replace",replace)
    with pytest.raises(OSError,match="injected"):corpus.publish_outputs(public_tmp,private_tmp,public,private)
    assert not private.exists() and not public.exists()

def test_synthetic_main_publishes_schema_valid_redacted_gap_and_private_mapping(tmp_path,monkeypatch):
    runner=tmp_path/"runner.exe";input_path=tmp_path/"input.png";runner.write_bytes(b"runner");input_path.write_bytes(b"input")
    public=tmp_path/"public";private=tmp_path/"private"
    @corpus.contextmanager
    def pinned(_runner,_input): yield runner,input_path
    monkeypatch.setattr(corpus,"pinned_common_sources",pinned)
    monkeypatch.setattr(corpus,"bind_locator",lambda inventory,locator:{"ntsc-rs":(tmp_path,{},[])})
    # The real corpus/local-locator.json is a machine-local, git-ignored file, so
    # a fresh checkout/CI has none. bind_locator is mocked above, so only the
    # schema-validating load must succeed: write a minimal valid locator to tmp.
    locator_path=tmp_path/"local-locator.json";locator_path.write_text(json.dumps({"schema_version":1,"plugins":{"ntsc-rs":{"source_root":"C:/local/ntsc-rs","aex":{"path":"ntsc-rs.aex","sha256":"a"*64,"size_bytes":1}}}}),encoding="utf-8")
    identity=lambda path,sha,size:{"path":path,"sha256":sha,"size_bytes":size}
    identities={"aex":identity("plugin/private-name.aex","a"*64,1),"dependencies":[],"input":identity("inputs/private.png","b"*64,1),"runner":identity("runner/private.exe","c"*64,1),"workers":[identity(f"target/worker-{i}.exe",str(i+1)*64,1) for i in range(3)]}
    report={"identities":identities};result={"classification":"missing_suite","selector":{"render_path":"classic","error_code":7},"missing_suites":[{"name":"PF World Suite","version":2}]};report_bytes=b'{"private":"local-only"}\n'
    monkeypatch.setattr(corpus,"execute_case",lambda *args:(report,result,corpus.hashlib.sha256(report_bytes).hexdigest(),{"manifest.json":b"{}\n","report.json":report_bytes,"run.json":b"{}\n"}))
    monkeypatch.setattr(corpus.sys,"argv",["run-real-aex-corpus.py","--inventory",str(ROOT/"corpus/real-aex-public.json"),"--locator",str(locator_path),"--matrix",str(ROOT/"corpus/common-matrix.json"),"--runner",str(runner),"--input",str(input_path),"--out",str(public),"--private-evidence-out",str(private),"--case-id","case-000001"])
    assert corpus.main()==0
    gaps=json.loads((public/"reproducible-gaps.json").read_text(encoding="utf-8"));Draft202012Validator(json.loads((ROOT/"schemas/real-aex-gaps.schema.json").read_text(encoding="utf-8"))).validate(gaps)
    encoded=(public/"reproducible-gaps.json").read_text(encoding="utf-8")+next((public/"evidence").glob("*.json")).read_text(encoding="utf-8")
    assert "ntsc-rs" not in encoded and "private-name" not in encoded and "case-000001" not in encoded and "0.5" not in encoded
    assert json.loads((private/"gap-map.json").read_text(encoding="utf-8"))["mapping"][0]["case_ids"]==["case-000001"]
    assert corpus.hashlib.sha256(next((public/"evidence").glob("*.json")).read_bytes()).hexdigest()==gaps["records"][0]["evidence"]["sha256"]

