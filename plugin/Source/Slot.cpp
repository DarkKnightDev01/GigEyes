#include "Slot.h"

Slot::Slot (std::unique_ptr<juce::AudioPluginInstance> p) : plugin (std::move (p))
{
    // Guitar plugins come stereo, mono-in/stereo-out (most amp sims) or mono: take the first
    // one the plugin accepts. Sidechains stay off.
    plugin->disableNonMainBuses();
    using Set = juce::AudioChannelSet;
    for (auto [in, out] : { std::pair { Set::stereo(), Set::stereo() },
                            std::pair { Set::mono(), Set::stereo() },
                            std::pair { Set::mono(), Set::mono() } })
    {
        BusesLayout layout;
        layout.inputBuses.add (in);
        layout.outputBuses.add (out);
        if (plugin->setBusesLayout (layout))
            break;
    }
}

void Slot::prepareToPlay (double sampleRate, int samplesPerBlock)
{
    plugin->setRateAndBufferSizeDetails (sampleRate, samplesPerBlock);
    plugin->prepareToPlay (sampleRate, samplesPerBlock);
    auto channels = juce::jmax (1, plugin->getTotalNumInputChannels(), plugin->getTotalNumOutputChannels());
    scratch.setSize (channels, samplesPerBlock);
    dry.setSize (2, samplesPerBlock);
    dryDelay.prepare ({ sampleRate, (juce::uint32) samplesPerBlock, 2 });
    smoothedMix.reset (sampleRate, 0.05);
    smoothedMix.setCurrentAndTargetValue (mix.load());
    setLatencySamples (plugin->getLatencySamples());
}

void Slot::processBlock (juce::AudioBuffer<float>& buffer, juce::MidiBuffer&)
{
    const auto n = buffer.getNumSamples();
    const auto ins = plugin->getTotalNumInputChannels();
    const auto outs = plugin->getTotalNumOutputChannels();

    // The dry path, held back by the plugin's latency (which can change, e.g. on a model switch).
    for (int ch = 0; ch < 2; ++ch)
        dry.copyFrom (ch, 0, buffer, ch, 0, n);
    juce::dsp::AudioBlock<float> dryBlock (dry.getArrayOfWritePointers(), 2, (size_t) n);
    dryDelay.setDelay ((float) juce::jlimit (0, (1 << 16) - 1, plugin->getLatencySamples()));
    dryDelay.process (juce::dsp::ProcessContextReplacing<float> (dryBlock));

    if (bypassed.load())
    {
        for (int ch = 0; ch < 2; ++ch)
            buffer.copyFrom (ch, 0, dry, ch, 0, n);
        return;
    }

    juce::AudioBuffer<float> io (scratch.getArrayOfWritePointers(), scratch.getNumChannels(), n);
    io.clear();
    for (int ch = 0; ch < juce::jmin (ins, 2); ++ch)
        io.copyFrom (ch, 0, buffer, ch, 0, n);
    juce::MidiBuffer noMidi;
    plugin->processBlock (io, noMidi);

    for (int ch = 0; ch < 2; ++ch)
        buffer.copyFrom (ch, 0, io, outs >= 2 ? ch : 0, 0, n);

    smoothedMix.setTargetValue (mix.load());
    if (! smoothedMix.isSmoothing() && smoothedMix.getTargetValue() >= 1.0f)
        return;
    for (int i = 0; i < n; ++i)
    {
        const auto wet = smoothedMix.getNextValue();
        for (int ch = 0; ch < 2; ++ch)
            buffer.setSample (ch, i, wet * buffer.getSample (ch, i) + (1.0f - wet) * dry.getSample (ch, i));
    }
}

void IrStage::prepareToPlay (double sampleRate, int samplesPerBlock)
{
    convolution.prepare ({ sampleRate, (juce::uint32) samplesPerBlock, 2 });
}

void IrStage::processBlock (juce::AudioBuffer<float>& buffer, juce::MidiBuffer&)
{
    if (! loaded.load() || bypassed.load())
        return;
    juce::dsp::AudioBlock<float> block (buffer);
    convolution.process (juce::dsp::ProcessContextReplacing<float> (block));
}

bool IrStage::load (const juce::MemoryBlock& data, const juce::String& irName)
{
    juce::AudioFormatManager formats;
    formats.registerBasicFormats();
    std::unique_ptr<juce::AudioFormatReader> reader (
        formats.createReaderFor (std::make_unique<juce::MemoryInputStream> (data, false)));
    if (reader == nullptr || reader->lengthInSamples <= 0)
        return false;
    const auto stereo = reader->numChannels >= 2;
    juce::AudioBuffer<float> response (stereo ? 2 : 1, (int) reader->lengthInSamples);
    reader->read (&response, 0, response.getNumSamples(), 0, true, stereo);
    // Handed over as a buffer it owns: the from-memory overload only keeps the pointer and reads
    // it later on a background thread. Not normalised: a match EQ's level is part of the correction.
    convolution.loadImpulseResponse (std::move (response), reader->sampleRate,
                                     stereo ? juce::dsp::Convolution::Stereo::yes : juce::dsp::Convolution::Stereo::no,
                                     juce::dsp::Convolution::Trim::no,
                                     juce::dsp::Convolution::Normalise::no);
    wav = data;
    name = irName;
    loaded = true;
    return true;
}

void IrStage::clear()
{
    loaded = false;
    wav.reset();
    name.clear();
}
