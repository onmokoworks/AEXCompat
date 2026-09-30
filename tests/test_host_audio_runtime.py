from _native_selftest import run


def test_non_input_audio_parameters_receive_silence_and_are_reclaimed():
    run("host_audio_runtime_selftest.exe", "host_audio_runtime_selftest")
