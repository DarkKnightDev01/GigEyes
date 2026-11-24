#pragma once

#include "Processor.h"

class GigEyesEditor final : public juce::AudioProcessorEditor,
                            private juce::ChangeListener,
                            private juce::Timer
{
public:
    explicit GigEyesEditor (GigEyesProcessor&);
    ~GigEyesEditor() override;

    void paint (juce::Graphics&) override;
    void resized() override;

private:
    struct Row;

    void changeListenerCallback (juce::ChangeBroadcaster*) override;
    void timerCallback() override;
    void rebuildRows();
    void showAddMenu();
    void addFile (const juce::String& path);
    void choose (const juce::String& title, const juce::String& patterns, int mode,
                 std::function<void (const juce::File&)> then);
    void say (const juce::String& message);

    GigEyesProcessor& processor;

    juce::TextButton addButton { "Add plugin" }, irButton { "Load IR..." }, irClear { "Clear IR" };
    juce::ToggleButton irOn { "Match EQ on" };
    juce::Label irLabel;

    juce::Viewport viewport;
    juce::Component rowHolder;
    juce::OwnedArray<Row> rows;

    juce::TextButton record { "Record DI" }, saveDi { "Save DI..." }, loadDi { "Load DI..." },
        loadTarget { "Load target stem..." };
    juce::TextButton live { "Live input" }, targetOnly { "A: target stem" }, diThrough { "B: DI through chain" };
    juce::Label diLabel, targetLabel, status, latency;

    std::unique_ptr<juce::FileChooser> chooser;

    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (GigEyesEditor)
};
