// Renders one frame per requested effect while tools/capture_ae_private_callbacks.py
// holds a Frida hook on the host process (issue #985). Environment contract:
//   AEXCAP_OUT_DIR   directory that receives <name>.png per effect and result.json
//   AEXCAP_GO_FILE   the JSX waits for this file before touching the project
//                    (the driver creates it once its hooks are installed)
//   AEXCAP_INPUT     input image imported as the footage
//   AEXCAP_EFFECTS   "matchName|displayName|outName[|Param=value,Param=value];..."
// The project is a fresh, unsaved one; the script refuses a non-empty project,
// closes without saving and quits AE.
(function () {
    function env(name) {
        var value = $.getenv(name);
        if (!value) { throw new Error("missing environment variable: " + name); }
        return value;
    }
    function jsonString(value) {
        var escaped = String(value).replace(/\\/g, "\\\\").replace(/"/g, '\\"')
            .replace(/[\x00-\x1f]/g, function (ch) {
                var hex = ch.charCodeAt(0).toString(16);
                while (hex.length < 4) { hex = "0" + hex; }
                return "\\u" + hex;
            });
        return '"' + escaped + '"';
    }
    function writeText(path, text) {
        var file = new File(path);
        file.encoding = "UTF-8";
        if (!file.open("w")) { throw new Error("could not create " + path); }
        file.write(text);
        file.close();
    }
    var outDir = env("AEXCAP_OUT_DIR");
    var goFile = new File(env("AEXCAP_GO_FILE"));
    var logLines = [];
    function logLine(s) { logLines.push(jsonString(s)); }
    var safeToQuit = false;
    try {
        if (app.project && (app.project.numItems !== 0 || app.project.file !== null)) {
            throw new Error("refusing to modify a non-empty or saved After Effects project");
        }
        safeToQuit = true;
        if (!app.project) { app.newProject(); }
        var waited = 0;
        while (!goFile.exists && waited < 180000) { $.sleep(500); waited += 500; }
        logLine("go_wait_ms=" + waited + " go=" + goFile.exists);
        if (!goFile.exists) { throw new Error("go file never appeared"); }

        var inputFile = new File(env("AEXCAP_INPUT"));
        if (!inputFile.exists) { throw new Error("input image does not exist"); }
        var footage = app.project.importFile(new ImportOptions(inputFile));
        app.project.bitsPerChannel = 8;
        var effects = env("AEXCAP_EFFECTS").split(";");
        for (var i = 0; i < effects.length; i++) {
            var spec = effects[i].split("|");
            var comp = app.project.items.addComp("cap_" + i, footage.width, footage.height, 1.0, 10, 30);
            var layer = comp.layers.add(footage);
            var effect = null;
            try { effect = layer.property("ADBE Effect Parade").addProperty(spec[0]); } catch (e1) { effect = null; }
            if (!effect) { effect = layer.property("ADBE Effect Parade").addProperty(spec[1]); }
            if (!effect) { logLine("effect_missing " + spec[0]); continue; }
            var extra = spec.length > 3 ? spec[3] : "";
            if (extra) {
                var pairs = extra.split(",");
                for (var p = 0; p < pairs.length; p++) {
                    var kv = pairs[p].split("=");
                    var prop = effect.property(kv[0]);
                    if (prop) { prop.setValue(Number(kv[1])); logLine("param " + spec[2] + " " + kv[0] + "=" + prop.value); }
                    else { logLine("param_missing " + spec[2] + " " + kv[0]); }
                }
            }
            comp.time = 0;
            if (!/^[A-Za-z0-9_-]+$/.test(spec[2])) { throw new Error("outName must be [A-Za-z0-9_-]+: " + spec[2]); }
            var outFile = new File(outDir + "/" + spec[2] + ".png");
            comp.saveFrameToPng(comp.time, outFile);
            var w = 0;
            while (!outFile.exists && w < 60000) { $.sleep(500); w += 500; }
            logLine("rendered " + spec[2] + " match=" + effect.matchName + " name=" + effect.name + " exists=" + outFile.exists + " wait_ms=" + w);
        }
        writeText(outDir + "/result.json", '{"status":"captured","ae_version":' + jsonString(app.version) + ',"log":[' + logLines.join(",") + ']}');
    } catch (error) {
        try { writeText(outDir + "/result.json", '{"status":"failed","error":' + jsonString(String(error)) + ',"log":[' + logLines.join(",") + ']}'); } catch (_) {}
    } finally {
        if (safeToQuit) {
            if (app.project && app.project.file === null) { app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); }
            app.quit();
        }
    }
}());
