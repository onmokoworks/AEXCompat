import contextlib
import io
import json
import tempfile
import unittest
from pathlib import Path
from unittest import mock

from tools import labctl


class LabctlTests(unittest.TestCase):
    def setUp(self):
        self.temp = tempfile.TemporaryDirectory()
        self.root = Path(self.temp.name)
        self.tools = self.root / "tools"
        self.pipelines = self.root / "pipelines"
        self.target = self.root / "target"
        self.index = self.target / "artifact-index"
        for path in (self.tools, self.pipelines, self.index):
            path.mkdir(parents=True)
        self.patches = [
            mock.patch.object(labctl, "LAB_ROOT", self.root), mock.patch.object(labctl, "TOOLS_ROOT", self.tools),
            mock.patch.object(labctl, "PIPELINE_ROOT", self.pipelines), mock.patch.object(labctl, "TARGET_ROOT", self.target),
            mock.patch.object(labctl, "ARTIFACT_INDEX_ROOT", self.index),
        ]
        for patch in self.patches: patch.start()
        for name in ("first.py", "second.py"):
            (self.tools / name).write_text("pass\n", encoding="utf-8")

    def tearDown(self):
        for patch in reversed(self.patches): patch.stop()
        self.temp.cleanup()

    def manifest(self, stages):
        path = self.pipelines / "test.json"
        path.write_text(json.dumps({"pipeline_name":"test","schema_version":1,"stages":stages}), encoding="utf-8")
        return path

    def stage(self, sid, tool, inputs=None):
        return {"stage_id":sid,"tool":tool,"args":["--fixed","yes"],"inputs":inputs or {},"output_root":"target/out"}

    def call(self, command, pipeline, artifacts=None):
        argv=[command,"--pipeline",str(pipeline)]
        for value in artifacts or []: argv.extend(["--artifact",value])
        out,err=io.StringIO(),io.StringIO()
        with contextlib.redirect_stdout(out),contextlib.redirect_stderr(err): code=labctl.main(argv)
        return code,out.getvalue(),err.getvalue()

    def test_dry_run_is_deterministic_and_never_launches(self):
        artifact=self.root/"input.json"; artifact.write_text("{}",encoding="utf-8")
        pipeline=self.manifest([self.stage("one","first.py",{"source":"artifact:sample","mode":"literal:safe"})])
        with mock.patch.object(labctl.subprocess,"run") as run:
            first=self.call("dry-run",pipeline,[f"sample={artifact}"])
            second=self.call("dry-run",pipeline,[f"sample={artifact}"])
        self.assertEqual(first,second); self.assertEqual(0,first[0]); run.assert_not_called()
        command=json.loads(first[1])[0]["command"]
        self.assertEqual(["--mode","safe","--source",str(artifact.resolve())],command[-4:])

    def test_run_success_executes_two_stages_in_order(self):
        pipeline=self.manifest([self.stage("one","first.py"),self.stage("two","second.py")])
        with mock.patch.object(labctl.subprocess,"run",side_effect=[mock.Mock(returncode=0),mock.Mock(returncode=0)]) as run:
            code,_,_=self.call("run",pipeline)
        self.assertEqual(0,code); self.assertEqual(2,run.call_count)
        self.assertIn("first.py",run.call_args_list[0].args[0][1]); self.assertIn("second.py",run.call_args_list[1].args[0][1])
        self.assertFalse(run.call_args_list[0].kwargs["shell"])

    def test_run_stops_after_first_failure(self):
        pipeline=self.manifest([self.stage("one","first.py"),self.stage("two","second.py")])
        with mock.patch.object(labctl.subprocess,"run",return_value=mock.Mock(returncode=7)) as run:
            code,_,_=self.call("run",pipeline)
        self.assertEqual(7,code); self.assertEqual(1,run.call_count)

    def test_rejects_tool_path_separator(self):
        pipeline=self.manifest([self.stage("one","nested/first.py")])
        self.assertEqual(2,self.call("dry-run",pipeline)[0])

    def test_unresolved_artifact_fails_closed(self):
        pipeline=self.manifest([self.stage("one","first.py",{"source":"artifact:missing"})])
        self.assertEqual(2,self.call("dry-run",pipeline)[0])

    def test_latest_readiness_index_resolves_found_label(self):
        artifact=self.root/"source.json"; artifact.write_text("{}",encoding="utf-8")
        payload={"report_kind":"aex_artifact_index","artifacts":[{"label":"sample","found":True,"path":str(artifact)}]}
        (self.index/"1-readiness-index.local.json").write_text(json.dumps(payload),encoding="utf-8")
        pipeline=self.manifest([self.stage("one","first.py",{"source":"artifact:sample"})])
        code,out,_=self.call("dry-run",pipeline)
        self.assertEqual(0,code); self.assertIn(str(artifact.resolve()),json.loads(out)[0]["command"])


if __name__ == "__main__": unittest.main()
