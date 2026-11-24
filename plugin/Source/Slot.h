#pragma once

#include <juce_audio_formats/juce_audio_formats.h>
#include <juce_audio_processors/juce_audio_processors.h>
#include <juce_dsp/juce_dsp.h>

// The boilerplate every graph node needs: stereo in, stereo out, no programs, no editor.
struct Stage : juce::AudioProcessor
{
    Stage()
        : AudioProcessor (BusesProperties()
                              .withInput ("In", juce::AudioChannelSet::stereo())
                              .withOutput ("Out", juce::AudioChannelSet::stereo())) {}

    void releaseResources() override {}
    double getTailLengthSeconds() const override { return 0.0; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool hasEditor() const override { return false; }
    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}
    void getStateInformation (juce::MemoryBlock&) override {}
    void setStateInformation (const void*, int) override {}
};

// One hosted plugin in the chain, with bypass and wet/dry. The dry signal is delayed by the
// plugin's latency so the blend doesn't comb-filter.
struct Slot final : Stage
{
    explicit Slot (std::unique_ptr<juce::AudioPluginInstance>);

    const juce::String getName() const override { return plugin->getName(); }
    void prepareToPlay (double sampleRate, int blockSize) override;
    void releaseResources() override { plugin->releaseResources(); }
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;
    using AudioProcessor::processBlock;
    double getTailLengthSeconds() const override { return plugin->getTailLengthSeconds(); }

    const std::unique_ptr<juce::AudioPluginInstance> plugin;
    std::atomic<bool> bypassed { false };
    std::atomic<float> mix { 1.0f };

private:
    juce::AudioBuffer<float> scratch, dry;
    juce::dsp::DelayLine<float, juce::dsp::DelayLineInterpolationTypes::None> dryDelay { 1 << 16 };
    juce::SmoothedValue<float> smoothedMix { 1.0f };
};

// The last stage: the Match-EQ impulse response. Passes audio through untouched until one is
// loaded or while it's bypassed.
struct IrStage final : Stage
{
    const juce::String getName() const override { return "Match EQ"; }
    void prepareToPlay (double sampleRate, int blockSize) override;
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;
    using AudioProcessor::processBlock;

    // Takes a WAV file's bytes; false if they aren't a readable audio file.
    bool load (const juce::MemoryBlock& wav, const juce::String& name);
    void clear();

    // Message thread only.
    juce::MemoryBlock wav;
    juce::String name;
    std::atomic<bool> bypassed { false };

private:
    juce::dsp::Convolution convolution;
    std::atomic<bool> loaded { false };
};
