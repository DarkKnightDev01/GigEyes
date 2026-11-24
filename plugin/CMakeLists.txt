cmake_minimum_required(VERSION 3.22)
project(GigEyes VERSION 0.9.0 LANGUAGES C CXX)

set(CMAKE_CXX_STANDARD 20)
set(CMAKE_CXX_STANDARD_REQUIRED ON)

include(FetchContent)
# Set FETCHCONTENT_SOURCE_DIR_JUCE to a local JUCE checkout to build offline.
FetchContent_Declare(JUCE
    GIT_REPOSITORY https://github.com/juce-framework/JUCE.git
    GIT_TAG 8.0.9
    GIT_SHALLOW TRUE)
FetchContent_MakeAvailable(JUCE)

set(GIGEYES_SOURCES
    Source/Processor.cpp
    Source/Slot.cpp
    Source/Editor.cpp)

juce_add_plugin(GigEyes
    COMPANY_NAME "GigEyes"
    PLUGIN_MANUFACTURER_CODE Gige
    PLUGIN_CODE GgEy
    FORMATS VST3 Standalone
    PRODUCT_NAME "GigEyes"
    IS_SYNTH FALSE
    NEEDS_MIDI_INPUT FALSE
    NEEDS_MIDI_OUTPUT FALSE
    VST3_CATEGORIES Fx
    COPY_PLUGIN_AFTER_BUILD FALSE)

target_sources(GigEyes PRIVATE ${GIGEYES_SOURCES})

set(GIGEYES_DEFINITIONS
    JUCE_PLUGINHOST_VST3=1
    JUCE_VST3_CAN_REPLACE_VST2=0
    JUCE_WEB_BROWSER=0
    JUCE_USE_CURL=0)
set(GIGEYES_MODULES
    juce::juce_audio_utils
    juce::juce_audio_processors
    juce::juce_dsp)

target_compile_definitions(GigEyes PUBLIC ${GIGEYES_DEFINITIONS} JUCE_APPLICATION_NAME_STRING="GigEyes")
target_link_libraries(GigEyes
    PRIVATE ${GIGEYES_MODULES}
    PUBLIC juce::juce_recommended_config_flags juce::juce_recommended_warning_flags)

option(GIGEYES_TESTS "Build the hosting test and its test plugin" ON)
if(GIGEYES_TESTS)
    # A real VST3 for the test to host: one gain knob, nothing else.
    juce_add_plugin(TestGain
        COMPANY_NAME "GigEyes"
        PLUGIN_MANUFACTURER_CODE Gige
        PLUGIN_CODE GgTg
        FORMATS VST3
        PRODUCT_NAME "TestGain"
        COPY_PLUGIN_AFTER_BUILD FALSE)
    target_sources(TestGain PRIVATE tests/TestGain.cpp)
    target_compile_definitions(TestGain PUBLIC JUCE_WEB_BROWSER=0 JUCE_USE_CURL=0 JUCE_VST3_CAN_REPLACE_VST2=0)
    target_link_libraries(TestGain PRIVATE juce::juce_audio_utils
        PUBLIC juce::juce_recommended_config_flags)

    juce_add_console_app(GigEyesTests PRODUCT_NAME "GigEyesTests")
    target_sources(GigEyesTests PRIVATE tests/HostTest.cpp ${GIGEYES_SOURCES})
    get_target_property(TEST_GAIN_PATH TestGain_VST3 JUCE_PLUGIN_ARTEFACT_FILE)
    target_compile_definitions(GigEyesTests PRIVATE ${GIGEYES_DEFINITIONS}
        JUCE_STANDALONE_APPLICATION=1
        TEST_GAIN_PATH="${TEST_GAIN_PATH}")
    target_link_libraries(GigEyesTests PRIVATE ${GIGEYES_MODULES} juce::juce_recommended_config_flags)
    add_dependencies(GigEyesTests TestGain_VST3)
endif()
