#include "worker_report.hpp"

#include <cmath>
#include <iostream>
#include <limits>
#include <string>

int main() {
  aexcompat::worker_report::L2ReportContext report;
  report.status = "parameters_inspected";
  aexcompat::worker_report::ParameterSnapshot parameter;
  parameter.index = 1;
  parameter.has_numeric = true;
  parameter.valid_min = 0.0;
  parameter.valid_max = std::numeric_limits<double>::infinity();
  parameter.slider_min = -std::numeric_limits<double>::infinity();
  parameter.slider_max = 100.0;
  parameter.default_value = std::numeric_limits<double>::quiet_NaN();
  parameter.has_current = true;
  parameter.current_value = 25.0;
  parameter.component_count = 3;
  parameter.default_components = {0.0, std::numeric_limits<double>::infinity(), 1.0};
  parameter.current_components = {0.0, 0.5, std::numeric_limits<double>::quiet_NaN()};
  report.parameters.push_back(parameter);

  const std::string json = aexcompat::worker_report::serialize_l2_report(report);
  const bool passed =
      json.find("\"valid_max\":null") != std::string::npos &&
      json.find("\"slider_min\":null") != std::string::npos &&
      json.find("\"default\":null") != std::string::npos &&
      json.find("\"default_components\":[0,null,1]") != std::string::npos &&
      json.find("\"current_components\":[0,0.5,null]") != std::string::npos &&
      json.find(":inf") == std::string::npos &&
      json.find(":-inf") == std::string::npos &&
      json.find(":nan") == std::string::npos;
  std::cout << "{\"worker_report_json\":\""
            << (passed ? "passed" : "failed")
            << "\",\"nonfinite_values\":\"null\"}\n";
  return passed ? 0 : 1;
}
