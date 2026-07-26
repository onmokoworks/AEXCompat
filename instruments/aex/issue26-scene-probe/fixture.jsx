(function () {
    function env(name) {
        return $.getenv(name);
    }

    app.beginUndoGroup("Issue26 Scene Probe Fixture");
    if (app.project) {
        app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES);
    }
    app.newProject();

    var folder = app.project.items.addFolder("Issue26 Folder");
    var comp = app.project.items.addComp(
        "Issue26 Scene Probe", 640, 360, 1.0, 10.0, 30.0);
    comp.parentFolder = folder;
    var childComp = app.project.items.addComp(
        "Issue26 Child Comp", 320, 180, 1.0, 5.0, 30.0);
    childComp.parentFolder = folder;
    childComp.layers.addSolid(
        [0.8, 0.3, 0.2], "Issue26 Child Footage", 320, 180, 1.0, 5.0);

    var solid = comp.layers.addSolid(
        [0.2, 0.4, 0.8], "Issue26 Footage", 640, 360, 1.0, 10.0);
    solid.source.parentFolder = folder;
    solid.threeDLayer = true;
    solid.property("ADBE Transform Group").property("ADBE Position")
        .setValueAtTime(0.0, [240, 180, 0]);
    solid.property("ADBE Transform Group").property("ADBE Position")
        .setValueAtTime(1.0, [400, 180, 50]);
    var position = solid.property("ADBE Transform Group").property("ADBE Position");
    position.setInterpolationTypeAtKey(
        1, KeyframeInterpolationType.BEZIER, KeyframeInterpolationType.BEZIER);
    position.setTemporalEaseAtKey(
        1, [new KeyframeEase(0, 33), new KeyframeEase(0, 33),
            new KeyframeEase(0, 33)],
        [new KeyframeEase(0, 33), new KeyframeEase(0, 33),
            new KeyframeEase(0, 33)]);

    var parent = comp.layers.addNull(10.0);
    parent.name = "Issue26 Parent";
    parent.threeDLayer = true;
    solid.parent = parent;

    var camera = comp.layers.addCamera("Issue26 Camera", [320, 180]);
    camera.property("ADBE Transform Group").property("ADBE Position")
        .setValue([320, 180, -800]);
    var zoom = camera.property("ADBE Camera Options Group")
        .property("ADBE Camera Zoom");
    zoom.setValueAtTime(0.0, 700);
    zoom.setValueAtTime(1.0, 900);
    zoom.setInterpolationTypeAtKey(
        1, KeyframeInterpolationType.BEZIER, KeyframeInterpolationType.BEZIER);
    zoom.setTemporalEaseAtKey(
        1, [new KeyframeEase(0, 33)], [new KeyframeEase(0, 33)]);

    solid.Effects.addProperty("ADBE Slider Control");
    solid.Effects.addProperty("ADBE Easy Levels");

    var mask = solid.Masks.addProperty("ADBE Mask Atom");
    mask.name = "Issue26 Mask";
    var shape = new Shape();
    shape.vertices = [[100, 80], [540, 80], [540, 280], [100, 280]];
    shape.inTangents = [[0, 0], [0, 0], [0, 0], [0, 0]];
    shape.outTangents = [[0, 0], [0, 0], [0, 0], [0, 0]];
    shape.closed = true;
    mask.property("ADBE Mask Shape").setValueAtTime(0.0, shape);
    shape.vertices = [[120, 90], [520, 90], [520, 270], [120, 270]];
    mask.property("ADBE Mask Shape").setValueAtTime(1.0, shape);

    comp.openInViewer();
    solid.selected = true;
    app.endUndoGroup();

    var metadataPath = env("ISSUE26_SCENE_FIXTURE_METADATA");
    if (metadataPath) {
        var file = new File(metadataPath);
        file.encoding = "UTF-8";
        if (file.open("w")) {
            file.write(JSON.stringify({
                schema_version: 1,
                fixture: "issue26-scene-probe",
                project_count: 1,
                folder_count: 1,
                footage_count: 2,
                comp_count: 2,
                layer_count: 4,
                effect_count: 2,
                mask_count: 1,
                position_keyframes: 2,
                mask_keyframes: 2,
                camera_zoom_keyframes: 2
            }));
            file.close();
        }
    }

    app.scheduleTask(
        "app.project.close(CloseOptions.DO_NOT_SAVE_CHANGES); app.quit();",
        12000, false);
}());
