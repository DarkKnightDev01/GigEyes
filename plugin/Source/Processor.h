#pragma once

#include "Slot.h"

// GigEyes.vst3: hosts the user's own VST3 plugins in series, then the Match-EQ IR.
//
//   Audio In -> slot 0 -> slot 1 -> ... -> Match EQ -> Audio Out
//
// Everything that changes the chain runs on the message thread; the audio thread only renders.
class GigEyesProcessor final : public juce::AudioProcessor,
                               public juce::ChangeBroadcaster,
                               private juce::Timer
{
public:
    using NodeID = juce::AudioProcessorGraph::NodeID;

    GigEyesProcessor();
    ~GigEyesProcessor() override;

    // --- Audio
    bool isBusesLayoutSupported (const BusesLayout&) const override;
    void prepareToPlay (double sampleRate, int blockSize) override;
    void releaseResources() override;
    void processBlock (juce::AudioBuffer<float>&, juce::MidiBuffer&) override;
    using AudioProcessor::processBlock;

    // --- Chain (message thread)
    // One entry per slot in signal order. A plugin that failed to load on restore stays as a
    // "missing" entry that keeps its saved state, so saving again doesn't lose it.
    struct Entry
    {
        NodeID node;                              // invalid when missing
        std::unique_ptr<juce::XmlElement> saved;  // the slot as saved, when missing
        juce::String missingName, error;
        // As it was loaded; the instance's own description can name a file the loader won't take back.
        juce::PluginDescription description;
    };
    const std::vector<Entry>& chain() const { return entries; }
    Slot* slot (NodeID) const;
    IrStage& ir() const { return *irStage; }

    // Loads every plugin type in a .vst3 file; most have one. Empty with `error` set on failure.
    juce::OwnedArray<juce::PluginDescription> typesIn (const juce::String& file, juce::String& error);
    bool add (const juce::PluginDescription&, juce::String& error);
    void remove (size_t index);
    void move (size_t index, int delta);
    void setBypassed (size_t index, bool);
    void setMix (size_t index, float);
    void loadIr (const juce::File&, juce::String& error);
    void clearIr();
    void setIrBypassed (bool);

    void showEditor (NodeID);
    void closeEditor (NodeID);

    // --- DI capture and A/B audition
    static constexpr double maxDiSeconds = 8.0;
    enum class Listen { live, target, di }; // live input / target stem alone / DI through the chain
    void startRecording();
    void stopRecording();
    bool isRecording() const { return recording.load(); }
    double diSeconds() const;
    bool saveDi (const juce::File&, juce::String& error);
    bool loadDi (const juce::File&, juce::String& error);
    bool loadTarget (const juce::File&, juce::String& error);
    double targetSeconds() const;
    juce::String targetName() const { return targetFile; }
    void setListen (Listen);
    Listen listening() const { return listen.load(); }

    // --- State
    void getStateInformation (juce::MemoryBlock&) override;
    void setStateInformation (const void*, int) override;

    // --- Boilerplate
    juce::AudioProcessorEditor* createEditor() override;
    bool hasEditor() const override { return true; }
    const juce::String getName() const override { return "GigEyes"; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    double getTailLengthSeconds() const override;
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}

private:
    std::unique_ptr<juce::AudioPluginInstance> instantiate (const juce::PluginDescription&, juce::String& error);
    NodeID host (std::unique_ptr<juce::AudioPluginInstance>);
    void rewire();
    void restore (std::unique_ptr<juce::XmlElement>);
    void removeAll();
    void timerCallback() override;
    int chainLatency() const;
    double rate() const { return getSampleRate() > 0 ? getSampleRate() : 48000.0; }
    // Reads an audio file into one buffer at the current sample rate, at most `seconds` long.
    bool readAudio (const juce::File&, double seconds, juce::AudioBuffer<float>&, juce::String& error) const;

    juce::AudioPluginFormatManager formats;
    juce::AudioProcessorGraph graph;
    NodeID input, output, irNode;
    IrStage* irStage = nullptr;
    std::vector<Entry> entries;
    // Held while the chain changes or is saved, in case a host saves from another thread.
    juce::CriticalSection chainLock;

    struct Window;
    std::map<juce::uint32, std::unique_ptr<Window>> windows;

    // Audio-thread buffers, swapped from the message thread under `audioLock`.
    juce::SpinLock audioLock;
    juce::AudioBuffer<float> di { 1, 0 }, target { 2, 0 };
    int diLength = 0, playPosition = 0;
    std::atomic<bool> recording { false }, restartPlayback { false };
    std::atomic<int> diRecorded { 0 };
    std::atomic<Listen> listen { Listen::live };
    juce::String targetFile;

    JUCE_DECLARE_WEAK_REFERENCEABLE (GigEyesProcessor)
    JUCE_DECLARE_NON_COPYABLE_WITH_LEAK_DETECTOR (GigEyesProcessor)
};
