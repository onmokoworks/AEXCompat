(function () {
    function requiredEnv(name) {
        var value = $.getenv(name);
        if (!value) {
            throw new Error("missing environment variable: " + name);
        }
        return value;
    }

    var projectFile = new File(requiredEnv("AEXCOMPAT_AE_ORACLE_PROJECT"));
    var outputFile = new File(requiredEnv("AEXCOMPAT_AE_ORACLE_OUTPUT"));
    var resultFile = new File(requiredEnv("AEXCOMPAT_AE_ORACLE_RENDER_RESULT"));
    var errorText = null;
    try {
        if (!projectFile.exists) {
            throw new Error("oracle project does not exist");
        }
        if (outputFile.exists) {
            throw new Error("refusing to overwrite oracle output");
        }
        app.open(projectFile);
        if (!app.project || app.project.renderQueue.numItems !== 1) {
            throw new Error("oracle project must contain exactly one render item");
        }
        var outputModule = app.project.renderQueue.item(1).outputModule(1);
        if (outputModule.file.fsName !== outputFile.fsName) {
            throw new Error("render queue output does not match requested output");
        }
        app.project.renderQueue.render();
        var renderedFile = outputFile;
        if (!renderedFile.exists) {
            var sequenceFiles = outputFile.parent.getFiles(outputFile.name + "*");
            if (sequenceFiles.length === 1 && sequenceFiles[0] instanceof File) {
                renderedFile = sequenceFiles[0];
            }
        }
        if (!renderedFile.exists) {
            throw new Error("render queue completed without producing output");
        }
    } catch (error) {
        errorText = String(error);
    } finally {
        resultFile.encoding = "UTF-8";
        if (resultFile.open("w")) {
            resultFile.write(errorText === null ? renderedFile.fsName : errorText);
            resultFile.close();
        }
        if (app.project) {
            app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
        }
        app.quit();
    }
}());
