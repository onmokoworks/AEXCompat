extern "C" __declspec(dllimport) int issue60_transitive_value();

extern "C" __declspec(dllexport) int issue60_delay_value() {
  return issue60_transitive_value();
}
