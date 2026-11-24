#include "Processor.h"
#include "Editor.h"

using IO = juce::AudioProcessorGraph::AudioGraphIOProcessor;
using Update = juce::AudioProcessorGraph::UpdateKind;

// A hosted plugin's own editor in a window of its own.
struct GigEyesProcessor::Window final : juce::DocumentWindow
{
    Window (juce::AudioProcessor& p, std::function<void()> onClose)
        : DocumentWindow (p.getName(), juce::Colours::black, DocumentWindow::closeButton),
          close (std::move (onClose))
    {
        setUsingNativeTitleBar (true);
        auto* editor = p.hasEditor() ? p.createEditorIfNeeded() : nullptr;
        if (editor == nullptr)
            editor = new juce::GenericAudioProcessorEditor (p);
        setContentOwned (editor, true);
        setResizable (editor->isResizable(), false);
        centreWithSize (getWidth(), getHeight());
        setVisible (true);
        toFront (true);
    }

    void closeButtonPressed() override { close(); }

    std::function<void()> close;
};

GigEyesProcessor::GigEyesProcessor()
    : AudioProcessor (BusesProperties()
                          .withInput ("Input", juce::AudioChannelSet::stereo())
                          .withOutput ("Output", juce::AudioChannelSet::stereo()))
{
    formats.addFormat (new juce::VST3PluginFormat());
    // The graph's I/O nodes take their channel count from the graph when added.
    graph.setPlayConfigDetails (2, 2, 48000.0, 512);
    input = graph.addNode (std::make_unique<IO> (IO::audioInputNode))->nodeID;
    output = graph.addNode (std::make_unique<IO> (IO::audioOutputNode))->nodeID;
    auto stage = std::make_unique<IrStage>();
    irStage = stage.get();
    irNode = graph.addNode (std::move (stage))->nodeID;
    rewire();
    startTimer (1000);
}

GigEyesProcessor::~GigEyesProcessor()
{
    stopTimer();
    windows.clear(); // editors go before the plugins they show
    graph.clear();
}

bool GigEyesProcessor::isBusesLayoutSupported (const BusesLayout& layout) const
{
    const auto in = layout.getMainInputChannelSet();
    return layout.getMainOutputChannelSet() == juce::AudioChannelSet::stereo()
        && (in == juce::AudioChannelSet::mono() || in == juce::AudioChannelSet::stereo());
}

void GigEyesProcessor::prepareToPlay (double sampleRate, int samplesPerBlock)
{
    graph.setPlayConfigDetails (2, 2, sampleRate, samplesPerBlock);
    graph.prepareToPlay (sampleRate, samplesPerBlock);
    setLatencySamples (chainLatency());
}

void GigEyesProcessor::releaseResources() { graph.releaseResources(); }

void GigEyesProcessor::processBlock (juce::AudioBuffer<float>& buffer, juce::MidiBuffer& midi)
{
    juce::ScopedNoDenormals noDenormals;
    const auto n = buffer.getNumSamples();

    if (recording.load())
    {
        const juce::SpinLock::ScopedTryLockType lock (audioLock);
        if (lock.isLocked())
        {
            const auto k = juce::jmin (n, di.getNumSamples() - diLength);
            di.copyFrom (0, diLength, buffer, 0, 0, k);
            diLength += k;
            diRecorded = diLength;
            if (diLength >= di.getNumSamples())
                recording = false;
        }
    }

    if (getTotalNumInputChannels() == 1)
        buffer.copyFrom (1, 0, buffer, 0, 0, n);

    const auto mode = listen.load();
    if (mode != Listen::live)
    {
        const juce::SpinLock::ScopedTryLockType lock (audioLock);
        auto& source = mode == Listen::target ? target : di;
        const auto length = mode == Listen::target ? target.getNumSamples() : diLength;
        if (! lock.isLocked() || length == 0)
        {
            buffer.clear();
            return;
        }
        if (restartPlayback.exchange (false) || playPosition >= length)
            playPosition = 0;
        for (int done = 0; done < n;)
        {
            const auto k = juce::jmin (n - done, length - playPosition);
            for (int ch = 0; ch < 2; ++ch)
                buffer.copyFrom (ch, done, source, juce::jmin (ch, source.getNumChannels() - 1), playPosition, k);
            done += k;
            playPosition = (playPosition + k) % length;
        }
        if (mode == Listen::target)
            return; // the stem as it is, for comparing against
    }

    graph.processBlock (buffer, midi);
}

// --- Chain

Slot* GigEyesProcessor::slot (NodeID id) const
{
    auto* node = graph.getNodeForId (id);
    return node != nullptr ? dynamic_cast<Slot*> (node->getProcessor()) : nullptr;
}

juce::OwnedArray<juce::PluginDescription> GigEyesProcessor::typesIn (const juce::String& file, juce::String& error)
{
    juce::OwnedArray<juce::PluginDescription> types;
    formats.getFormat (0)->findAllTypesForFile (types, file);
    if (types.isEmpty())
        error = "No VST3 plugin could be read from " + file;
    return types;
}

std::unique_ptr<juce::AudioPluginInstance> GigEyesProcessor::instantiate (const juce::PluginDescription& d, juce::String& error)
{
    if (d.manufacturerName == "GigEyes" && d.name == "GigEyes")
    {
        error = "GigEyes can't host itself.";
        return nullptr;
    }
    return formats.createPluginInstance (d, rate(), getBlockSize() > 0 ? getBlockSize() : 512, error);
}

GigEyesProcessor::NodeID GigEyesProcessor::host (std::unique_ptr<juce::AudioPluginInstance> plugin)
{
    return graph.addNode (std::make_unique<Slot> (std::move (plugin)), std::nullopt, Update::none)->nodeID;
}

bool GigEyesProcessor::add (const juce::PluginDescription& d, juce::String& error)
{
    auto plugin = instantiate (d, error);
    if (plugin == nullptr)
    {
        if (error.isEmpty())
            error = "Couldn't load " + d.name;
        return false;
    }
    {
        const juce::ScopedLock lock (chainLock);
        entries.push_back ({ host (std::move (plugin)), nullptr, {}, {}, d });
    }
    rewire();
    return true;
}

void GigEyesProcessor::remove (size_t index)
{
    {
        const juce::ScopedLock lock (chainLock);
        if (index >= entries.size())
            return;
        if (auto id = entries[index].node; id != NodeID())
        {
            closeEditor (id);
            graph.removeNode (id, Update::none);
        }
        entries.erase (entries.begin() + (std::ptrdiff_t) index);
    }
    rewire();
}

void GigEyesProcessor::move (size_t index, int delta)
{
    const auto to = (int) index + delta;
    {
        const juce::ScopedLock lock (chainLock);
        if (index >= entries.size() || to < 0 || to >= (int) entries.size())
            return;
        std::swap (entries[index], entries[(size_t) to]);
    }
    rewire();
}

void GigEyesProcessor::setBypassed (size_t index, bool b)
{
    if (index < entries.size())
        if (auto* s = slot (entries[index].node))
            s->bypassed = b;
    sendChangeMessage();
}

void GigEyesProcessor::setMix (size_t index, float m)
{
    if (index < entries.size())
        if (auto* s = slot (entries[index].node))
            s->mix = juce::jlimit (0.0f, 1.0f, m);
}

void GigEyesProcessor::loadIr (const juce::File& file, juce::String& error)
{
    juce::MemoryBlock data;
    if (! file.loadFileAsData (data) || ! irStage->load (data, file.getFileName()))
        error = "Couldn't read " + file.getFileName() + " as an impulse response.";
    sendChangeMessage();
}

void GigEyesProcessor::clearIr()
{
    irStage->clear();
    sendChangeMessage();
}

void GigEyesProcessor::setIrBypassed (bool b)
{
    irStage->bypassed = b;
    sendChangeMessage();
}

// Audio In -> each loaded slot in order -> Match EQ -> Audio Out.
void GigEyesProcessor::rewire()
{
    {
        const juce::ScopedLock lock (chainLock);
        for (auto& c : graph.getConnections())
            graph.removeConnection (c, Update::none);
        auto link = [this] (NodeID from, NodeID to) {
            for (int ch = 0; ch < 2; ++ch)
                graph.addConnection ({ { from, ch }, { to, ch } }, Update::none);
        };
        auto previous = input;
        for (auto& e : entries)
            if (e.node != NodeID())
            {
                link (previous, e.node);
                previous = e.node;
            }
        link (previous, irNode);
        link (irNode, output);
    }
    graph.rebuild();
    setLatencySamples (chainLatency());
    sendChangeMessage();
}

int GigEyesProcessor::chainLatency() const
{
    int total = 0;
    for (auto& e : entries)
        if (auto* s = slot (e.node))
            total += s->plugin->getLatencySamples();
    return total;
}

double GigEyesProcessor::getTailLengthSeconds() const
{
    double total = 0;
    for (auto& e : entries)
        if (auto* s = slot (e.node))
            total += s->plugin->getTailLengthSeconds();
    return total;
}

// Hosted plugins can change their latency at any time (an amp suite switching models), so the
// total reported to the host is checked once a second.
void GigEyesProcessor::timerCallback()
{
    if (const auto latency = chainLatency(); latency != getLatencySamples())
    {
        setLatencySamples (latency);
        sendChangeMessage();
    }
}

void GigEyesProcessor::showEditor (NodeID id)
{
    auto* s = slot (id);
    if (s == nullptr)
        return;
    if (auto it = windows.find (id.uid); it != windows.end())
    {
        it->second->toFront (true);
        return;
    }
    juce::WeakReference<GigEyesProcessor> self (this);
    windows[id.uid] = std::make_unique<Window> (*s->plugin, [self, id] {
        // Closed after the click is handled, not from inside the window's own callback.
        juce::MessageManager::callAsync ([self, id] {
            if (self != nullptr)
                self->closeEditor (id);
        });
    });
}

void GigEyesProcessor::closeEditor (NodeID id) { windows.erase (id.uid); }

// --- DI and audition

void GigEyesProcessor::startRecording()
{
    {
        const juce::SpinLock::ScopedLockType lock (audioLock);
        di.setSize (1, (int) (maxDiSeconds * rate()));
        di.clear();
        diLength = 0;
    }
    diRecorded = 0;
    listen = Listen::live;
    recording = true;
}

void GigEyesProcessor::stopRecording() { recording = false; }

double GigEyesProcessor::diSeconds() const { return diRecorded.load() / rate(); }

double GigEyesProcessor::targetSeconds() const { return target.getNumSamples() / rate(); }

void GigEyesProcessor::setListen (Listen l)
{
    restartPlayback = true;
    listen = l;
}

bool GigEyesProcessor::readAudio (const juce::File& file, double seconds, juce::AudioBuffer<float>& out, juce::String& error) const
{
    juce::AudioFormatManager audioFormats;
    audioFormats.registerBasicFormats();
    std::unique_ptr<juce::AudioFormatReader> reader (audioFormats.createReaderFor (file));
    if (reader == nullptr || reader->lengthInSamples <= 0)
    {
        error = "Couldn't read " + file.getFileName() + " as audio.";
        return false;
    }
    const auto length = (int) std::min (reader->lengthInSamples, (juce::int64) (seconds * reader->sampleRate));
    const auto channels = out.getNumChannels();
    juce::AudioBuffer<float> raw (channels, length);
    reader->read (&raw, 0, length, 0, true, channels > 1);
    if (reader->numChannels == 1)
        for (int ch = 1; ch < channels; ++ch)
            raw.copyFrom (ch, 0, raw, 0, 0, length);

    const auto ratio = reader->sampleRate / rate();
    if (std::abs (ratio - 1.0) < 1e-6)
    {
        out = std::move (raw);
        return true;
    }
    const auto resampled = (int) (length / ratio);
    out.setSize (channels, resampled);
    for (int ch = 0; ch < channels; ++ch)
    {
        juce::LagrangeInterpolator interpolator;
        interpolator.process (ratio, raw.getReadPointer (ch), out.getWritePointer (ch), resampled);
    }
    return true;
}

bool GigEyesProcessor::saveDi (const juce::File& file, juce::String& error)
{
    juce::AudioBuffer<float> copy;
    {
        const juce::SpinLock::ScopedLockType lock (audioLock);
        copy.makeCopyOf (di);
        copy.setSize (1, diLength, true);
    }
    if (copy.getNumSamples() == 0)
    {
        error = "Nothing recorded yet.";
        return false;
    }
    file.deleteFile();
    std::unique_ptr<juce::OutputStream> stream = file.createOutputStream();
    auto writer = stream != nullptr
        ? juce::WavAudioFormat().createWriterFor (stream, juce::AudioFormatWriterOptions {}
                                                              .withSampleRate (rate())
                                                              .withNumChannels (1)
                                                              .withBitsPerSample (24))
        : nullptr;
    if (writer == nullptr || ! writer->writeFromAudioSampleBuffer (copy, 0, copy.getNumSamples()))
    {
        error = "Couldn't write " + file.getFullPathName();
        return false;
    }
    return true;
}

bool GigEyesProcessor::loadDi (const juce::File& file, juce::String& error)
{
    juce::AudioBuffer<float> loaded (1, 0);
    // A loaded DI can run longer than a recorded one; 30 s keeps memory small.
    if (! readAudio (file, 30.0, loaded, error))
        return false;
    recording = false;
    const juce::SpinLock::ScopedLockType lock (audioLock);
    di = std::move (loaded);
    diLength = di.getNumSamples();
    diRecorded = diLength;
    restartPlayback = true;
    return true;
}

bool GigEyesProcessor::loadTarget (const juce::File& file, juce::String& error)
{
    juce::AudioBuffer<float> loaded (2, 0);
    // shortcut: the first 2 minutes only; enough for a riff, raise it if whole songs are wanted.
    if (! readAudio (file, 120.0, loaded, error))
        return false;
    {
        const juce::SpinLock::ScopedLockType lock (audioLock);
        target = std::move (loaded);
        restartPlayback = true;
    }
    targetFile = file.getFileName();
    return true;
}

// --- State: every slot's plugin, its own saved state, bypass and mix, then the IR's bytes.

void GigEyesProcessor::getStateInformation (juce::MemoryBlock& dest)
{
    juce::XmlElement root ("GigEyes");
    root.setAttribute ("version", 1);
    {
        const juce::ScopedLock lock (chainLock);
        for (auto& e : entries)
        {
            if (e.saved != nullptr)
            {
                root.addChildElement (new juce::XmlElement (*e.saved));
                continue;
            }
            auto* s = slot (e.node);
            if (s == nullptr)
                continue;
            auto* x = root.createNewChildElement ("Slot");
            x->addChildElement (e.description.createXml().release());
            juce::MemoryBlock state;
            s->plugin->getStateInformation (state);
            x->setAttribute ("state", state.toBase64Encoding());
            x->setAttribute ("bypassed", s->bypassed.load());
            x->setAttribute ("mix", (double) s->mix.load());
        }
    }
    if (irStage->wav.getSize() > 0)
    {
        auto* x = root.createNewChildElement ("IR");
        x->setAttribute ("name", irStage->name);
        x->setAttribute ("data", irStage->wav.toBase64Encoding());
        x->setAttribute ("bypassed", irStage->bypassed.load());
    }
    copyXmlToBinary (root, dest);
}

void GigEyesProcessor::setStateInformation (const void* data, int size)
{
    std::shared_ptr<juce::XmlElement> xml = getXmlFromBinary (data, size);
    if (xml == nullptr || ! xml->hasTagName ("GigEyes"))
        return;
    // Plugins are created and their state set on the message thread, as VST3 expects.
    if (juce::MessageManager::getInstance()->isThisTheMessageThread())
        restore (std::make_unique<juce::XmlElement> (*xml));
    else
        juce::MessageManager::callAsync ([self = juce::WeakReference<GigEyesProcessor> (this), xml] {
            if (self != nullptr)
                self->restore (std::make_unique<juce::XmlElement> (*xml));
        });
}

void GigEyesProcessor::removeAll()
{
    const juce::ScopedLock lock (chainLock);
    windows.clear();
    for (auto& e : entries)
        if (e.node != NodeID())
            graph.removeNode (e.node, Update::none);
    entries.clear();
}

void GigEyesProcessor::restore (std::unique_ptr<juce::XmlElement> root)
{
    removeAll();
    for (auto* x : root->getChildWithTagNameIterator ("Slot"))
    {
        juce::PluginDescription d;
        juce::String error;
        auto* xmlDesc = x->getChildByName ("PLUGIN");
        auto plugin = xmlDesc != nullptr && d.loadFromXml (*xmlDesc) ? instantiate (d, error) : nullptr;
        if (plugin == nullptr)
        {
            if (! juce::File::isAbsolutePath (d.fileOrIdentifier) || ! juce::File (d.fileOrIdentifier).exists())
                error = "not found at " + d.fileOrIdentifier;
            const juce::ScopedLock lock (chainLock);
            entries.push_back ({ {}, std::make_unique<juce::XmlElement> (*x),
                                 d.name.isNotEmpty() ? d.name : juce::String ("Unknown plugin"),
                                 error.isNotEmpty() ? error : juce::String ("couldn't be loaded"), d });
            continue;
        }
        juce::MemoryBlock state;
        if (state.fromBase64Encoding (x->getStringAttribute ("state")) && state.getSize() > 0)
            plugin->setStateInformation (state.getData(), (int) state.getSize());
        const juce::ScopedLock lock (chainLock);
        const auto id = host (std::move (plugin));
        auto* s = slot (id);
        s->bypassed = x->getBoolAttribute ("bypassed");
        s->mix = (float) x->getDoubleAttribute ("mix", 1.0);
        entries.push_back ({ id, nullptr, {}, {}, d });
    }

    irStage->clear();
    if (auto* x = root->getChildByName ("IR"))
    {
        juce::MemoryBlock wav;
        if (wav.fromBase64Encoding (x->getStringAttribute ("data")))
            irStage->load (wav, x->getStringAttribute ("name"));
        irStage->bypassed = x->getBoolAttribute ("bypassed");
    }
    rewire();
}

juce::AudioProcessorEditor* GigEyesProcessor::createEditor() { return new GigEyesEditor (*this); }

juce::AudioProcessor* JUCE_CALLTYPE createPluginFilter() { return new GigEyesProcessor(); }
