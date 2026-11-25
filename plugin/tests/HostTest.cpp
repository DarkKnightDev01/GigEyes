// Hosts the real TestGain.vst3 inside GigEyesProcessor and checks what reaches the output.
// Run: GigEyesTests (exit code 0 = all passed).
#include "../Source/Editor.h"

namespace
{
int failures = 0;

void check (bool ok, const juce::String& what)
{
    std::printf ("%s  %s\n", ok ? "ok  " : "FAIL", what.toRawUTF8());
    if (! ok)
        ++failures;
}

bool near (float a, float b) { return std::abs (a - b) < 1e-3f; }

constexpr double rate = 48000.0;
constexpr int block = 256;

void prepare (GigEyesProcessor& p)
{
    p.setRateAndBufferSizeDetails (rate, block);
    p.prepareToPlay (rate, block);
}

// Feeds a constant 1.0 for long enough to pass latency and smoothing; returns the last sample.
float settle (GigEyesProcessor& p, int blocks = 40)
{
    juce::AudioBuffer<float> buffer (2, block);
    juce::MidiBuffer midi;
    for (int i = 0; i < blocks; ++i)
    {
        for (int ch = 0; ch < 2; ++ch)
            juce::FloatVectorOperations::fill (buffer.getWritePointer (ch), 1.0f, block);
        p.processBlock (buffer, midi);
    }
    return buffer.getSample (1, block - 1);
}

// The convolution swaps a new IR in on a background thread, so keep processing until it lands.
float settleTo (GigEyesProcessor& p, float want)
{
    float got = 0;
    for (int i = 0; i < 100 && ! near (got, want); ++i)
    {
        got = settle (p, 4);
        juce::Thread::sleep (10);
    }
    return got;
}

juce::AudioProcessorParameter& gainOf (GigEyesProcessor& p, size_t index)
{
    return *p.slot (p.chain()[index].node)->plugin->getParameters()[0];
}

juce::MemoryBlock save (GigEyesProcessor& p)
{
    juce::MemoryBlock m;
    p.getStateInformation (m);
    return m;
}

juce::File writeWav (const juce::String& name, const std::vector<float>& samples)
{
    auto file = juce::File::getSpecialLocation (juce::File::tempDirectory).getChildFile (name);
    file.deleteFile();
    std::unique_ptr<juce::OutputStream> stream = file.createOutputStream();
    auto writer = juce::WavAudioFormat().createWriterFor (stream, juce::AudioFormatWriterOptions {}
                                                                      .withSampleRate (rate)
                                                                      .withNumChannels (1)
                                                                      .withBitsPerSample (32));
    juce::AudioBuffer<float> b (1, (int) samples.size());
    b.copyFrom (0, 0, samples.data(), (int) samples.size());
    writer->writeFromAudioSampleBuffer (b, 0, b.getNumSamples());
    return file;
}

// GigEyesTests <a.vst3> <b.vst3> ...: hosts the user's own plugins in series and reports, per
// plugin, whether it loads, opens its editor, keeps up in real time and restores its state.
int realPlugins (const juce::StringArray& paths)
{
    GigEyesProcessor p;
    prepare (p);
    for (auto& path : paths)
    {
        juce::String error;
        auto types = p.typesIn (path, error);
        check (! types.isEmpty() && p.add (*types[0], error), "loads " + path + " " + error);
    }
    if (p.chain().empty())
        return 1;

    for (auto& e : p.chain())
    {
        auto* s = p.slot (e.node);
        const auto before = juce::TopLevelWindow::getNumTopLevelWindows();
        p.showEditor (e.node);
        juce::MessageManager::getInstance()->runDispatchLoopUntil (1500);
        check (juce::TopLevelWindow::getNumTopLevelWindows() == before + 1,
               s->plugin->getName() + (s->plugin->hasEditor() ? ": its own editor opens" : ": no editor of its own, generic one opens"));
        p.closeEditor (e.node);
        juce::MessageManager::getInstance()->runDispatchLoopUntil (300);
    }

    // 10 s of a low E string-ish tone through the whole chain, timed against real time.
    constexpr int seconds = 10, blocks = (int) (seconds * rate / block);
    juce::AudioBuffer<float> buffer (2, block);
    juce::MidiBuffer midi;
    float peak = 0;
    bool finite = true;
    const auto start = juce::Time::getMillisecondCounterHiRes();
    for (int b = 0; b < blocks; ++b)
    {
        for (int i = 0; i < block; ++i)
            for (int ch = 0; ch < 2; ++ch)
                buffer.setSample (ch, i, 0.3f * std::sin (juce::MathConstants<float>::twoPi * 82.4f * (float) (b * block + i) / (float) rate));
        p.processBlock (buffer, midi);
        for (int ch = 0; ch < 2; ++ch)
            for (int i = 0; i < block; ++i)
            {
                const auto v = buffer.getSample (ch, i);
                finite = finite && std::isfinite (v);
                peak = std::max (peak, std::abs (v));
            }
    }
    const auto load = (juce::Time::getMillisecondCounterHiRes() - start) / (seconds * 10.0);
    check (finite, "output has no NaN/inf");
    check (peak > 1e-4f, "output not silent (peak " + juce::String (peak, 3) + ")");
    check (load < 50, "chain uses " + juce::String (load, 1) + "% of real time (under 50% is safe)");
    std::printf ("      latency reported to the host: %d samples\n", p.getLatencySamples());

    const auto state = save (p);
    GigEyesProcessor q;
    prepare (q);
    q.setStateInformation (state.getData(), (int) state.getSize());
    check (q.chain().size() == p.chain().size(), "every slot comes back after reload");
    for (size_t i = 0; i < std::min (p.chain().size(), q.chain().size()); ++i)
    {
        auto* a = p.slot (p.chain()[i].node);
        auto* b = q.slot (q.chain()[i].node);
        if (b == nullptr)
        {
            check (false, a->plugin->getName() + " restored: " + q.chain()[i].error);
            continue;
        }
        juce::MemoryBlock before, after;
        a->plugin->getStateInformation (before);
        b->plugin->getStateInformation (after);
        // Some plugins stamp their state with volatile data, so a byte difference is a warning only.
        std::printf ("%s  %s settings after reload (%d bytes)\n", before == after ? "ok  " : "WARN",
                     (a->plugin->getName() + (before == after ? ": identical" : ": differ")).toRawUTF8(), (int) after.getSize());
    }
    std::printf ("%s\n", failures == 0 ? "all passed" : "FAILED");
    return failures == 0 ? 0 : 1;
}
} // namespace

int main (int argc, char** argv)
{
    std::setvbuf (stdout, nullptr, _IONBF, 0);
    if (argc > 1)
    {
        juce::ScopedJuceInitialiser_GUI juce;
        juce::StringArray paths;
        for (int i = 1; i < argc; ++i)
            paths.add (argv[i]);
        return realPlugins (paths);
    }
    juce::ScopedJuceInitialiser_GUI juce;

    GigEyesProcessor p;
    prepare (p);
    check (near (settle (p), 1.0f), "empty chain passes audio through");

    juce::String error;
    auto types = p.typesIn (TEST_GAIN_PATH, error);
    check (types.size() == 1, "TestGain.vst3 scanned " + error);
    if (types.isEmpty())
        return 1;

    check (p.add (*types[0], error) && p.add (*types[0], error), "two plugins added " + error);
    check (p.chain().size() == 2, "chain has two slots");
    gainOf (p, 0).setValueNotifyingHost (0.5f);
    gainOf (p, 1).setValueNotifyingHost (0.5f);
    check (near (settle (p), 0.25f), "two gains of 0.5 in series give 0.25");
    check (p.getLatencySamples() == 128, "latency of both slots reported: " + juce::String (p.getLatencySamples()));

    p.setBypassed (1, true);
    check (near (settle (p), 0.5f), "bypassed slot passes audio through");

    p.setMix (0, 0.5f);
    p.setBypassed (1, false);
    check (near (settle (p), 0.375f), "50% mix blends dry and wet in time");
    p.setMix (0, 1.0f);

    const auto first = p.chain()[0].node, second = p.chain()[1].node;
    p.move (0, 1);
    check (p.chain()[0].node == second && p.chain()[1].node == first, "move swaps slots");
    gainOf (p, 0).setValueNotifyingHost (0.25f);
    check (near (settle (p), 0.125f), "audio follows the new order");

    // IR: one sample at 0.5 halves the signal.
    auto ir = writeWav ("gigeyes-test-ir.wav", { 0.5f });
    p.loadIr (ir, error);
    check (error.isEmpty(), "IR loaded " + error);
    const auto withIr = settleTo (p, 0.0625f);
    check (near (withIr, 0.0625f), "IR applied after the chain: " + juce::String (withIr));

    // State: a new instance from saved bytes sounds the same.
    p.setBypassed (0, true);
    const auto state = save (p);
    {
        GigEyesProcessor q;
        prepare (q);
        q.setStateInformation (state.getData(), (int) state.getSize());
        const auto ok = q.chain().size() == 2 && q.slot (q.chain()[0].node) != nullptr && q.slot (q.chain()[1].node) != nullptr;
        check (ok, "restored two slots: " + juce::String ((int) q.chain().size())
                       + (q.chain().empty() ? juce::String() : " " + q.chain()[0].error));
        if (! ok)
        {
            std::printf ("%s\n", juce::AudioProcessor::getXmlFromBinary (state.getData(), (int) state.getSize())->toString().substring (0, 1500).toRawUTF8());
            return 1;
        }
        check (q.slot (q.chain()[0].node)->bypassed.load(), "restored bypass");
        check (near (gainOf (q, 0).getValue(), 0.25f) && near (gainOf (q, 1).getValue(), 0.5f),
               "restored each plugin's own state");
        check (q.ir().name == "gigeyes-test-ir.wav", "restored IR");
        const auto restored = settleTo (q, 0.25f);
        check (near (restored, 0.25f), "restored instance sounds the same: " + juce::String (restored));
    }

    // A plugin that's gone keeps its slot and saved state.
    {
        auto xml = juce::AudioProcessor::getXmlFromBinary (state.getData(), (int) state.getSize());
        xml->getChildByName ("Slot")->getChildByName ("PLUGIN")->setAttribute ("file", "/nowhere/Gone.vst3");
        juce::MemoryBlock broken;
        juce::AudioProcessor::copyXmlToBinary (*xml, broken);

        GigEyesProcessor q;
        prepare (q);
        q.setStateInformation (broken.getData(), (int) broken.getSize());
        check (q.chain().size() == 2 && q.chain()[0].node == GigEyesProcessor::NodeID()
                   && q.chain()[0].missingName == "TestGain",
               "missing plugin shown as missing: " + q.chain()[0].error);
        check (near (settleTo (q, 0.25f), 0.25f), "rest of the chain still plays");
        const auto resaved = save (q);
        auto again = juce::AudioProcessor::getXmlFromBinary (resaved.getData(), (int) resaved.getSize());
        check (again->getChildByName ("Slot")->getChildByName ("PLUGIN")->getStringAttribute ("file")
                   == "/nowhere/Gone.vst3",
               "missing plugin's saved slot is kept on save");
    }

    // DI: record, save, load back, play through the chain; A plays the target untouched.
    {
        GigEyesProcessor q;
        prepare (q);
        q.startRecording();
        settle (q, 10);
        q.stopRecording();
        check (std::abs (q.diSeconds() - 10.0 * block / rate) < 1e-6, "recorded DI length");

        auto di = juce::File::getSpecialLocation (juce::File::tempDirectory).getChildFile ("gigeyes-test-di.wav");
        check (q.saveDi (di, error), "DI saved " + error);
        GigEyesProcessor r;
        prepare (r);
        check (r.loadDi (di, error) && std::abs (r.diSeconds() - q.diSeconds()) < 1e-6, "DI loaded back " + error);
        r.add (*types[0], error);
        gainOf (r, 0).setValueNotifyingHost (0.5f);
        r.setListen (GigEyesProcessor::Listen::di);
        juce::AudioBuffer<float> silence (2, block);
        juce::MidiBuffer midi;
        for (int i = 0; i < 4; ++i)
        {
            silence.clear();
            r.processBlock (silence, midi);
        }
        check (near (silence.getSample (0, block - 1), 0.5f), "B plays the DI through the chain");

        auto stem = writeWav ("gigeyes-test-stem.wav", std::vector<float> (4800, 0.8f));
        check (r.loadTarget (stem, error), "target loaded " + error);
        r.setListen (GigEyesProcessor::Listen::target);
        silence.clear();
        r.processBlock (silence, midi);
        check (near (silence.getSample (1, 10), 0.8f), "A plays the target stem untouched");
        di.deleteFile();
        stem.deleteFile();
    }
    // The editor builds a row per slot; GIGEYES_SNAPSHOT=<file.png> saves what it draws.
    {
        std::unique_ptr<juce::AudioProcessorEditor> editor (p.createEditor());
        check (editor != nullptr && editor->getWidth() > 0, "editor opens");
        if (const auto png = juce::SystemStats::getEnvironmentVariable ("GIGEYES_SNAPSHOT", {}); png.isNotEmpty())
        {
            juce::File file (png);
            file.deleteFile();
            juce::FileOutputStream out (file);
            juce::PNGImageFormat().writeImageToStream (editor->createComponentSnapshot (editor->getLocalBounds(), true, 2.0f), out);
        }
    }
    ir.deleteFile();

    std::printf ("%s\n", failures == 0 ? "all passed" : "FAILED");
    return failures == 0 ? 0 : 1;
}
