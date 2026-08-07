def test_pre_unload_hook_event_log_contract_covers_finish_and_destructor_paths():
    # A fake event log captures the lifecycle promised by the concrete call
    # ordering above. Repeated finish/destructor cleanup must not run the hook
    # or unload twice.
    events = []
    invoked = False
    module_live = True

    def quiesce_once():
        nonlocal invoked
        if not invoked:
            invoked = True
            events.append("hook")
        return True

    def capture_terminal_audit():
        quiesce_once()
        if "audit" not in events:
            events.append("audit")

    def unload_module():
        nonlocal module_live
        if module_live and quiesce_once():
            events.append("FreeLibrary")
            module_live = False

    def finish():
        capture_terminal_audit()
        unload_module()

    finish()
    finish()  # destructor fallback after an explicit finish
    assert events == ["hook", "audit", "FreeLibrary"]
