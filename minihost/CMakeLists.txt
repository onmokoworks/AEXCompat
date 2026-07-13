cmake_minimum_required(VERSION 3.20)
project(AEXCompatMinihost LANGUAGES CXX)

add_executable(aex_l1_worker src/main.cpp)
target_compile_features(aex_l1_worker PRIVATE cxx_std_17)
target_compile_definitions(aex_l1_worker PRIVATE UNICODE _UNICODE WIN32_LEAN_AND_MEAN NOMINMAX)
target_link_libraries(aex_l1_worker PRIVATE bcrypt)

