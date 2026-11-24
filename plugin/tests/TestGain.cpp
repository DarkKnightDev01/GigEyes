// A real VST3 for the hosting test: one gain knob, and 64 samples of latency so the test can
// check GigEyes reports it and lines up its dry path.
#include <juce_audio_processors/juce_audio_processors.h>

struct TestGain final : juce::AudioProcessor
{
    static constexpr int delay = 64;

    TestGain()
        : AudioProcessor (BusesProperties()
                              .withInput ("In", juce::AudioChannelSet::stereo())
                              .withOutput ("Out", juce::AudioChannelSet::stereo()))
    {
        addParameter (gain = new juce::AudioParameterFloat ({ "gain", 1 }, "Gain", 0.0f, 1.0f, 1.0f));
        setLatencySamples (delay);
    }

    void prepareToPlay (double, int) override
    {
        line.setSize (2, delay);
        line.clear();
        at = 0;
    }

    void processBlock (juce::AudioBuffer<float>& buffer, juce::MidiBuffer&) override
    {
        const auto g = gain->get();
        for (int i = 0; i < buffer.getNumSamples(); ++i)
        {
            for (int ch = 0; ch < 2; ++ch)
            {
                const auto in = buffer.getSample (ch, i);
                buffer.setSample (ch, i, g * line.getSample (ch, at));
                line.setSample (ch, at, in);
            }
            at = (at + 1) % delay;
        }
    }

    void getStateInformation (juce::MemoryBlock& m) override { juce::MemoryOutputStream (m, false).writeFloat (gain->get()); }
    void setStateInformation (const void* d, int n) override
    {
        *gain = juce::MemoryInputStream (d, (size_t) n, false).readFloat();
    }

    void releaseResources() override {}
    const juce::String getName() const override { return "TestGain"; }
    double getTailLengthSeconds() const override { return 0; }
    bool acceptsMidi() const override { return false; }
    bool producesMidi() const override { return false; }
    bool hasEditor() const override { return false; }
    juce::AudioProcessorEditor* createEditor() override { return nullptr; }
    int getNumPrograms() override { return 1; }
    int getCurrentProgram() override { return 0; }
    void setCurrentProgram (int) override {}
    const juce::String getProgramName (int) override { return {}; }
    void changeProgramName (int, const juce::String&) override {}

    juce::AudioParameterFloat* gain;
    juce::AudioBuffer<float> line;
    int at = 0;
};

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter() { return new TestGain(); }
