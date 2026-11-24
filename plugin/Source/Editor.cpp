#include "Editor.h"

namespace
{
constexpr int rowHeight = 36, gap = 8;
}

// One slot: name, open its editor, on/off, wet/dry, move, remove.
struct GigEyesEditor::Row final : juce::Component
{
    Row (GigEyesProcessor& p, size_t index) : processor (p), at (index)
    {
        const auto& e = p.chain()[index];
        auto* s = p.slot (e.node);
        name.setText (juce::String ((int) index + 1) + ". "
                          + (s != nullptr ? s->plugin->getName() : "Missing: " + e.missingName + " (" + e.error + ")"),
                      juce::dontSendNotification);
        for (auto* c : std::initializer_list<juce::Component*> { &name, &open, &on, &mix, &up, &down, &remove })
            addAndMakeVisible (c);

        open.setEnabled (s != nullptr);
        on.setEnabled (s != nullptr);
        mix.setEnabled (s != nullptr);
        on.setToggleState (s != nullptr && ! s->bypassed.load(), juce::dontSendNotification);
        mix.setSliderStyle (juce::Slider::LinearHorizontal);
        mix.setTextBoxStyle (juce::Slider::TextBoxRight, false, 48, 20);
        mix.setRange (0.0, 100.0, 1.0);
        mix.setTextValueSuffix ("%");
        mix.setValue (s != nullptr ? s->mix.load() * 100.0 : 100.0, juce::dontSendNotification);
        mix.setTooltip ("Wet/dry");

        open.onClick = [this] { processor.showEditor (processor.chain()[at].node); };
        on.onClick = [this] { processor.setBypassed (at, ! on.getToggleState()); };
        mix.onValueChange = [this] { processor.setMix (at, (float) mix.getValue() / 100.0f); };
        up.onClick = [this] { processor.move (at, -1); };
        down.onClick = [this] { processor.move (at, 1); };
        remove.onClick = [this] { processor.remove (at); };
    }

    void resized() override
    {
        auto r = getLocalBounds().reduced (0, 4);
        remove.setBounds (r.removeFromRight (70));
        down.setBounds (r.removeFromRight (60).reduced (2, 0));
        up.setBounds (r.removeFromRight (50).reduced (2, 0));
        mix.setBounds (r.removeFromRight (150));
        on.setBounds (r.removeFromRight (50));
        open.setBounds (r.removeFromRight (60).reduced (2, 0));
        name.setBounds (r);
    }

    GigEyesProcessor& processor;
    const size_t at;
    juce::Label name;
    juce::TextButton open { "Edit" }, up { "Up" }, down { "Down" }, remove { "Remove" };
    juce::ToggleButton on { "On" };
    juce::Slider mix;
};

GigEyesEditor::GigEyesEditor (GigEyesProcessor& p) : AudioProcessorEditor (p), processor (p)
{
    for (auto* c : std::initializer_list<juce::Component*> { &addButton, &irButton, &irClear, &irOn, &irLabel,
                                                             &viewport, &record, &saveDi, &loadDi, &loadTarget,
                                                             &live, &targetOnly, &diThrough, &diLabel,
                                                             &targetLabel, &status, &latency })
        addAndMakeVisible (c);
    viewport.setViewedComponent (&rowHolder, false);
    viewport.setScrollBarsShown (true, false);

    addButton.onClick = [this] { showAddMenu(); };
    irButton.onClick = [this] {
        choose ("Load a Match-EQ impulse response", "*.wav;*.aif;*.aiff", juce::FileBrowserComponent::canSelectFiles,
                [this] (const juce::File& f) {
                    juce::String error;
                    processor.loadIr (f, error);
                    say (error);
                });
    };
    irClear.onClick = [this] { processor.clearIr(); };
    irOn.onClick = [this] { processor.setIrBypassed (! irOn.getToggleState()); };

    record.onClick = [this] {
        if (processor.isRecording())
            processor.stopRecording();
        else
            processor.startRecording();
        say ({});
    };
    saveDi.onClick = [this] {
        choose ("Save the DI", "*.wav", juce::FileBrowserComponent::saveMode | juce::FileBrowserComponent::canSelectFiles
                                             | juce::FileBrowserComponent::warnAboutOverwriting,
                [this] (const juce::File& f) {
                    juce::String error;
                    processor.saveDi (f.withFileExtension ("wav"), error);
                    say (error.isEmpty() ? "Saved " + f.withFileExtension ("wav").getFileName() : error);
                });
    };
    loadDi.onClick = [this] {
        choose ("Load a DI recording", "*.wav;*.aif;*.aiff;*.flac", juce::FileBrowserComponent::canSelectFiles,
                [this] (const juce::File& f) {
                    juce::String error;
                    processor.loadDi (f, error);
                    say (error);
                });
    };
    loadTarget.onClick = [this] {
        choose ("Load the isolated guitar stem to match", "*.wav;*.aif;*.aiff;*.flac",
                juce::FileBrowserComponent::canSelectFiles, [this] (const juce::File& f) {
                    juce::String error;
                    processor.loadTarget (f, error);
                    say (error);
                });
    };

    using Listen = GigEyesProcessor::Listen;
    for (auto [button, mode] : { std::pair { &live, Listen::live }, std::pair { &targetOnly, Listen::target },
                                 std::pair { &diThrough, Listen::di } })
    {
        button->setClickingTogglesState (true);
        button->setRadioGroupId (1);
        button->onClick = [this, button, mode] {
            if (button->getToggleState())
                processor.setListen (mode);
        };
    }
    live.setToggleState (processor.listening() == Listen::live, juce::dontSendNotification);
    targetOnly.setToggleState (processor.listening() == Listen::target, juce::dontSendNotification);
    diThrough.setToggleState (processor.listening() == Listen::di, juce::dontSendNotification);

    status.setColour (juce::Label::textColourId, juce::Colours::orange);
    processor.addChangeListener (this);
    rebuildRows();
    setResizable (true, true);
    setResizeLimits (640, 420, 1600, 1200);
    setSize (720, 520);
    timerCallback(); // labels filled before the first paint
    startTimerHz (10);
}

GigEyesEditor::~GigEyesEditor() { processor.removeChangeListener (this); }

void GigEyesEditor::paint (juce::Graphics& g)
{
    g.fillAll (getLookAndFeel().findColour (juce::ResizableWindow::backgroundColourId));
    g.setColour (juce::Colours::white);
    g.setFont (juce::FontOptions (18.0f, juce::Font::bold));
    g.drawText ("GigEyes", getLocalBounds().reduced (12).removeFromTop (24), juce::Justification::left);
    g.setFont (juce::FontOptions (13.0f));
    g.setColour (juce::Colours::grey);
    if (processor.chain().empty())
        g.drawText ("Add your amp sim, pedals and effects in signal order.", viewport.getBounds(),
                    juce::Justification::centred);
}

void GigEyesEditor::resized()
{
    auto r = getLocalBounds().reduced (12);
    r.removeFromTop (28);

    auto bar = r.removeFromTop (30);
    addButton.setBounds (bar.removeFromLeft (110));
    bar.removeFromLeft (gap);
    irButton.setBounds (bar.removeFromLeft (100));
    bar.removeFromLeft (gap);
    irClear.setBounds (bar.removeFromLeft (80));
    bar.removeFromLeft (gap);
    irOn.setBounds (bar.removeFromLeft (120));
    irLabel.setBounds (bar);
    r.removeFromTop (gap);

    auto bottom = r.removeFromBottom (4 * 30 + 3 * gap);
    viewport.setBounds (r.withTrimmedBottom (gap));
    rowHolder.setSize (viewport.getWidth() - viewport.getScrollBarThickness(), rowHeight * rows.size());
    for (int i = 0; i < rows.size(); ++i)
        rows[i]->setBounds (0, i * rowHeight, rowHolder.getWidth(), rowHeight);

    auto line = [&bottom] {
        auto l = bottom.removeFromTop (30);
        bottom.removeFromTop (gap);
        return l;
    };
    auto di = line();
    record.setBounds (di.removeFromLeft (100));
    di.removeFromLeft (gap);
    saveDi.setBounds (di.removeFromLeft (90));
    di.removeFromLeft (gap);
    loadDi.setBounds (di.removeFromLeft (90));
    di.removeFromLeft (gap);
    diLabel.setBounds (di);

    auto t = line();
    loadTarget.setBounds (t.removeFromLeft (150));
    t.removeFromLeft (gap);
    targetLabel.setBounds (t);

    auto l = line();
    const auto third = l.getWidth() / 3;
    live.setBounds (l.removeFromLeft (third).reduced (2, 0));
    targetOnly.setBounds (l.removeFromLeft (third).reduced (2, 0));
    diThrough.setBounds (l.reduced (2, 0));

    auto s = line();
    latency.setBounds (s.removeFromRight (160));
    status.setBounds (s);
}

void GigEyesEditor::changeListenerCallback (juce::ChangeBroadcaster*) { rebuildRows(); }

void GigEyesEditor::rebuildRows()
{
    rows.clear();
    for (size_t i = 0; i < processor.chain().size(); ++i)
        rowHolder.addAndMakeVisible (rows.add (new Row (processor, i)));
    const auto& ir = processor.ir();
    irOn.setToggleState (! ir.bypassed.load(), juce::dontSendNotification);
    irLabel.setText (ir.name.isEmpty() ? "No IR loaded" : ir.name, juce::dontSendNotification);
    resized();
    repaint();
}

void GigEyesEditor::timerCallback()
{
    const auto recording = processor.isRecording();
    record.setButtonText (recording ? "Stop" : "Record DI");
    const auto seconds = processor.diSeconds();
    diLabel.setText (recording ? juce::String::formatted ("Recording %.1f / %.0f s, play your riff", seconds,
                                                          GigEyesProcessor::maxDiSeconds)
                     : seconds > 0 ? juce::String::formatted ("DI: %.1f s", seconds)
                                   : juce::String ("No DI yet"),
                     juce::dontSendNotification);
    targetLabel.setText (processor.targetName().isEmpty()
                             ? juce::String ("No target stem")
                             : processor.targetName() + juce::String::formatted (" (%.1f s)", processor.targetSeconds()),
                         juce::dontSendNotification);
    latency.setText ("Latency: " + juce::String (processor.getLatencySamples()) + " samples",
                     juce::dontSendNotification);
}

void GigEyesEditor::showAddMenu()
{
    juce::VST3PluginFormat vst3;
    auto files = vst3.searchPathsForPlugins (vst3.getDefaultLocationsToSearch(), true, false);
    files.sortNatural();
    juce::PopupMenu menu;
    for (int i = 0; i < files.size(); ++i)
        menu.addItem (i + 1, juce::File (files[i]).getFileNameWithoutExtension());
    if (! files.isEmpty())
        menu.addSeparator();
    menu.addItem (files.size() + 1, "Browse for a .vst3...");
    menu.showMenuAsync (juce::PopupMenu::Options().withTargetComponent (addButton),
                        [this, files] (int picked) {
                            if (picked <= 0)
                                return;
                            if (picked <= files.size())
                                return addFile (files[picked - 1]);
                            choose ("Add a VST3 plugin", "*.vst3",
                                    juce::FileBrowserComponent::canSelectFiles
                                        | juce::FileBrowserComponent::canSelectDirectories,
                                    [this] (const juce::File& f) { addFile (f.getFullPathName()); });
                        });
}

void GigEyesEditor::addFile (const juce::String& path)
{
    juce::String error;
    auto types = processor.typesIn (path, error);
    if (types.isEmpty())
        return say (error);
    if (types.size() == 1)
    {
        processor.add (*types[0], error);
        return say (error);
    }
    // A shell file with several plugins in it: pick one.
    juce::PopupMenu menu;
    for (int i = 0; i < types.size(); ++i)
        menu.addItem (i + 1, types[i]->name);
    auto shared = std::make_shared<juce::OwnedArray<juce::PluginDescription>> (std::move (types));
    menu.showMenuAsync (juce::PopupMenu::Options().withTargetComponent (addButton), [this, shared] (int picked) {
        if (picked <= 0)
            return;
        juce::String e;
        processor.add (*(*shared)[picked - 1], e);
        say (e);
    });
}

void GigEyesEditor::choose (const juce::String& title, const juce::String& patterns, int mode,
                            std::function<void (const juce::File&)> then)
{
    chooser = std::make_unique<juce::FileChooser> (title, juce::File(), patterns);
    const auto save = (mode & juce::FileBrowserComponent::saveMode) != 0;
    chooser->launchAsync (save ? mode : mode | juce::FileBrowserComponent::openMode, [then] (const juce::FileChooser& fc) {
        if (auto f = fc.getResult(); f != juce::File())
            then (f);
    });
}

void GigEyesEditor::say (const juce::String& message) { status.setText (message, juce::dontSendNotification); }
