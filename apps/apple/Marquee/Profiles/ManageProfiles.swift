import SwiftUI

/// The owner's list of profiles, each opening its edit page, and Add.
struct ManageProfiles: View {
    let core: CoreClient
    @Environment(ProfileSession.self) private var profiles
    @Environment(\.metrics) private var metrics

    var body: some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.s) {
                ScreenTitle(text: "Manage profiles")
                if let snapshot = profiles.snapshot {
                    if snapshot.current?.isOwner != true {
                        StateLine(symbol: "lock", text: "Only the main profile manages profiles. You can edit your own from Settings.")
                    } else {
                        ForEach(snapshot.profiles) { profile in
                            NavigationLink { EditProfile(profileId: profile.id) } label: {
                                ProfileRow(stored: profile.avatar, name: profile.name, detail: ProfileRules.detail(profile))
                            }
                            .buttonStyle(.row)
                        }
                        if snapshot.profiles.count < snapshot.max {
                            NavigationLink { EditProfile(profileId: nil) } label: {
                                SettingsRow(symbol: "plus", title: "Add profile", detail: "A new profile starts with your addons and no PIN.")
                            }
                            .buttonStyle(.row)
                        } else {
                            StateLine(symbol: "users", text: "This account has as many profiles as it can hold (\(snapshot.max)).")
                        }
                    }
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
    }
}

/// A profile as a Settings row: its avatar, its name and one line.
struct ProfileRow: View {
    let stored: String
    let name: String
    let detail: String

    var body: some View {
        HStack(spacing: Theme.Space.s) {
            ProfileAvatar(stored, name: name, size: 40)
            VStack(alignment: .leading, spacing: 3) {
                Text(name).textFont(Theme.type.headline, weight: .semibold, relativeTo: .headline)
                Text(detail)
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(Theme.Palette.muted)
            }
            Spacer()
            Glyph("chevron-right").foregroundStyle(Theme.Palette.muted)
        }
        .foregroundStyle(Theme.Palette.text)
        .padding(Theme.Space.s)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        .contentShape(Rectangle())
        .accessibilityElement(children: .combine)
    }
}

/// Add a profile, or edit one: the name, the picture, the PIN
/// changes the core allows, and Delete for the owner on another profile.
struct EditProfile: View {
    /// nil adds a profile.
    let profileId: String?
    @Environment(ProfileSession.self) private var profiles
    @Environment(\.dismiss) private var dismiss
    @Environment(\.metrics) private var metrics
    @State private var name = ""
    @FocusState private var nameFocused: Bool
    @State private var glyph = "letter"
    @State private var color = "ember"
    /// The user picked a picture: only then is `avatar` sent, so a
    /// newer app's avatar survives a rename.
    @State private var touched = false
    @State private var filled = false
    @State private var said: (text: String, ok: Bool)?
    @State private var busy = false
    @State private var flow: PinFlow?
    @State private var confirmDelete = false
    /// A delete is out: Delete and Keep wait for its answer.
    @State private var deleting = false
    /// The delete confirmation opens on Keep, so a held Select or Return never
    /// deletes; Keep gives the focus back to Delete profile.
    @FocusState private var deleteFocus: DeleteFocus?
    private enum DeleteFocus { case open, keep }
    /// The profile whose PIN a newer version stored: it can only be removed.
    @State private var unreadable: String?
    @FocusState private var removeFocused: Bool
    /// A PIN removal waiting for its confirmation (`PinAction.confirms`).
    @State private var removal: (action: PinAction, target: ProfileInfo)?
    @State private var confirmLeave = false
    @Environment(\.tabShown) private var shown
    @Environment(\.scenePhase) private var phase

    var body: some View {
        let editor = pinDialog(leaveGuard(ScrollViewReader { proxy in page(proxy) }))
        #if os(tvOS)
        editor
        #else
        // Outermost, so both dialogs draw in the key blue, not the root's white tint (DECISION, Confirmation).
        editor.tint(Theme.Palette.key)
        #endif
    }

    /// The edit page: the form, the PIN section and Delete.
    private func page(_ proxy: ScrollViewProxy) -> some View {
        ScrollView(.vertical) {
            VStack(alignment: .leading, spacing: Theme.Space.m) {
                if let snapshot = profiles.snapshot, let active = snapshot.current {
                    let target = profileId.flatMap { id in snapshot.profiles.first { $0.id == id } }
                    if profileId != nil && target == nil {
                        StateLine(symbol: "circle-alert", text: "That profile was removed on another device.")
                    } else if !(active.isOwner || target?.id == active.id) {
                        StateLine(symbol: "lock", text: "Only the main profile can change other profiles.")
                    } else if let flow, let target {
                        PinKeypad(
                            title: flow.prompt, cancelTitle: "Cancel",
                            submit: { pin in await step(pin, target) },
                            cancel: { self.flow = nil })
                    } else {
                        ScreenTitle(text: title(target, active))
                        form(snapshot)
                        if let target { pin(target, active) }
                        if let target, active.isOwner, !target.isOwner, target.id != active.id {
                            if confirmDelete {
                                deleteConfirm(target).id("delete")
                            } else {
                                Button("Delete profile") { confirmDelete = true }
                                    .buttonStyle(DestructiveTextStyle())
                                    .focused($deleteFocus, equals: .open)
                            }
                        }
                    }
                }
            }
            .frame(maxWidth: 760, alignment: .leading)
            .frame(maxWidth: .infinity, alignment: .leading)
            .padding(.horizontal, metrics.gutter)
            .padding(.vertical, Theme.Space.l)
        }
        .scrollIndicators(.hidden)
        .background(Theme.Palette.bg)
        .topScrim()
        #if os(macOS)
        // The Go menu's Back takes the same ask as the Back button.
        .environment(\.leaveAsk, leaveAsk)
        #endif
        .onAppear(perform: fill)
        // A half-done PIN change is given up when the app leaves the front:
        // the steps forget the current and new PINs.
        .onChange(of: phase) { _, phase in if phase != .active { flow = nil } }
        // The confirmation comes into view when it opens.
        .onChange(of: confirmDelete) { _, open in
            if open { withAnimation(Theme.Motion.panel) { proxy.scrollTo("delete", anchor: .center) } }
        }
    }

    /// Back and the Go menu's Back ask while the page holds edits.
    private var leaveAsk: (() -> Void)? {
        guard dirty else { return nil }
        return { leave() }
    }

    /// Edits are not lost to a stray Back, on every platform with a back (not TV).
    @ViewBuilder private func leaveGuard(_ content: some View) -> some View {
        #if os(tvOS)
        content
        #else
        content
            // The system back (and its swipe) gives way to the Back below, which asks.
            #if os(iOS)
            // The Mac's is hidden in `BackOnlyWhenShown`, which reads `leaveAsk`.
            .navigationBarBackButtonHidden(backButtonHidden(shown: shown, holdsEdits: dirty))
            #endif
            .toolbar { leaveBack }
            .confirmationDialog("You have unsaved changes", isPresented: $confirmLeave, titleVisibility: .visible) {
                // Saving leaves once it worked; a failure stays here with its line, nothing lost.
                Button("Save changes") { save(thenLeave: true) }
                Button("Discard changes") { dismiss() }
                Button("Keep editing", role: .cancel) {}
            }
        #endif
    }

    #if !os(tvOS)
    @ToolbarContentBuilder private var leaveBack: some ToolbarContent {
        if dirty, shown {
            ToolbarItem(placement: .navigation) {
                Button(action: leave) { Label("Back", glyph: "chevron-left") }
                    // The system back is a chevron alone; the control keeps its shape when the first edit swaps them.
                    .labelStyle(.iconOnly)
            }
        }
    }
    #endif

    private func pinDialog(_ content: some View) -> some View {
        content
            .confirmationDialog(pinTitle, isPresented: removing, titleVisibility: .visible, presenting: removal) { removal in
                Button(label(removal.action, removal.target)) { removePin(removal.action, removal.target) }
                Button("Cancel", role: .cancel) {}
                    #if !os(tvOS)
                    // Return never removes a PIN.
                    .keyboardShortcut(.defaultAction)
                    #endif
            } message: { removal in
                Text("Anyone can then open \(removal.action == .removeOther ? removal.target.name : "this profile") without it.")
            }
    }

    private var pinTitle: String {
        removal.map { label($0.action, $0.target) + "?" } ?? "Remove PIN?"
    }

    private var removing: Binding<Bool> {
        Binding(get: { removal != nil }, set: { if !$0 { removal = nil } })
    }

    private func deleteConfirm(_ target: ProfileInfo) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            Text("Delete \(target.name)? Their library, progress and settings are removed from this device now and from your other devices when they next sync.")
                .textFont(Theme.type.body)
                .foregroundStyle(Theme.Palette.text)
                .fixedSize(horizontal: false, vertical: true)
            HStack(spacing: 12) {
                Button("Keep") {
                    confirmDelete = false
                    // Once Delete profile is back on screen.
                    Task { deleteFocus = .open }
                }
                .buttonStyle(PillButtonStyle(prominent: true))
                #if !os(tvOS)
                .keyboardShortcut(.defaultAction)
                #endif
                .focused($deleteFocus, equals: .keep)
                // Sent once: both wait for the answer.
                .disabled(deleting)
                Button("Delete \(target.name)", role: .destructive) { delete(target) }
                    .buttonStyle(DestructiveTextStyle())
                    .disabled(deleting)
            }
        }
        .padding(Theme.Space.s)
        .frame(maxWidth: .infinity, alignment: .leading)
        .glass(RoundedRectangle(cornerRadius: Theme.Radius.card, style: .continuous))
        #if os(tvOS)
        .focusSection()
        #endif
        .onAppear { deleteFocus = .keep }
    }

    private func leave() { confirmLeave = true }

    private var dirty: Bool {
        let stored = profileId.flatMap { id in profiles.snapshot?.profiles.first { $0.id == id }?.name }
        return ProfileRules.isDirty(name: name, stored: stored, touched: touched)
    }

    private func title(_ target: ProfileInfo?, _ active: ProfileInfo) -> String {
        guard let target else { return "Add profile" }
        return target.id == active.id ? "Your profile" : "Edit \(target.name)"
    }

    /// The fields are filled once; later snapshots do not overwrite what is typed.
    private func fill() {
        guard !filled, let snapshot = profiles.snapshot else { return }
        filled = true
        if let target = profileId.flatMap({ id in snapshot.profiles.first { $0.id == id } }) {
            name = target.name
            let avatar = ProfileRules.parseAvatar(target.avatar)
            glyph = avatar.glyph
            color = avatar.color ?? "ember"
        } else {
            let avatar = ProfileRules.parseAvatar(ProfileRules.nextAvatar(snapshot.profiles.map(\.avatar)))
            glyph = avatar.glyph
            color = avatar.color ?? "ember"
        }
    }

    private func form(_ snapshot: ProfilesSnapshot) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.s) {
            HStack(spacing: Theme.Space.s) {
                ProfileAvatar(ProfileRules.Avatar(glyph: glyph, color: color), name: name, size: 72)
                VStack(alignment: .leading, spacing: 6) {
                    caps("Name")
                    TextField("Name", text: $name)
                        .textFieldStyle(.plain)
                        .textFont(Theme.type.body)
                        .padding(.horizontal, 16)
                        .padding(.vertical, 12)
                        .focused($nameFocused)
                        .fieldChrome(focused: nameFocused)
                        .onChange(of: name) { _, typed in if typed.count > 30 { name = String(typed.prefix(30)) } }
                }
            }
            caps("Picture")
            ScrollView(.horizontal) {
                HStack(spacing: 10) {
                    ForEach(ProfileRules.glyphs, id: \.self) { item in
                        choice(selected: item == glyph, label: ProfileRules.glyphName(item)) {
                            glyph = item
                            touched = true
                        } content: {
                            ProfileAvatar(ProfileRules.Avatar(glyph: item, color: color), name: name, size: 48)
                        }
                    }
                }
                .padding(4)
            }
            .scrollIndicators(.hidden)
            // The focus ring and the lift draw past the row; the scroll view must not cut them.
            .scrollClipDisabled()
            if let said {
                Label(said.text, glyph: said.ok ? "circle-check" : "triangle-alert")
                    .textFont(Theme.type.callout, relativeTo: .callout)
                    .foregroundStyle(said.ok ? Theme.Palette.muted : Theme.Palette.text)
            }
            Button(profileId == nil ? "Add profile" : "Save") { save() }
                .buttonStyle(PillButtonStyle(prominent: true))
                .disabled(busy)
        }
    }

    private func caps(_ text: String) -> some View {
        Text(caps: text)
            .textFont(Theme.type.caption, weight: .semibold, relativeTo: .caption)
            .tracking(1.6)
            .foregroundStyle(Theme.Palette.muted)
    }

    private func choice(selected: Bool, label: String, pick: @escaping () -> Void, @ViewBuilder content: () -> some View) -> some View {
        Button(action: pick) {
            content()
                .padding(3)
                .overlay(RoundedRectangle(cornerRadius: 14, style: .continuous).strokeBorder(selected ? Theme.Palette.text : .clear, lineWidth: 2))
        }
        .buttonStyle(FocusPlainStyle(shape: RoundedRectangle(cornerRadius: 14, style: .continuous)))
        .accessibilityLabel(label)
        .accessibilityAddTraits(selected ? .isSelected : [])
    }

    @ViewBuilder private func pin(_ target: ProfileInfo, _ active: ProfileInfo) -> some View {
        VStack(alignment: .leading, spacing: Theme.Space.xs) {
            Text("PIN").display(Theme.type.title2).foregroundStyle(Theme.Palette.text)
            let actions = PinAction.allowed(for: target, by: active, unreadable: unreadable == target.id)
            if actions == [.removeUnreadable] {
                StateLine(symbol: "triangle-alert", text: ProfileRules.problem("pinUnreadable", fallback: ""))
                Button("Remove PIN") { begin(.removeUnreadable, target) }
                    .buttonStyle(PillButtonStyle(prominent: true))
                    .disabled(busy)
                    .focused($removeFocused)
                    .onAppear { removeFocused = true }
            } else if target.syncing {
                StateLine(symbol: "clock", text: "Waiting for this profile's lock to arrive from your other devices.")
            } else if actions.isEmpty {
                StateLine(symbol: "info", text: "\(target.name) has no PIN. They can set one from their own profile.")
            } else {
                FlowRow(spacing: 12) {
                    ForEach(actions, id: \.self) { action in
                        Button(label(action, target)) { begin(action, target) }
                            .buttonStyle(PillButtonStyle())
                            .disabled(busy)
                    }
                }
            }
        }
    }

    private func label(_ action: PinAction, _ target: ProfileInfo) -> String {
        switch action {
        case .set: "Set a PIN"
        case .change: "Change PIN"
        case .remove: "Remove PIN"
        case .removeOther: "Remove \(target.name)'s PIN"
        case .removeUnreadable: "Remove PIN"
        }
    }

    private func begin(_ action: PinAction, _ target: ProfileInfo) {
        said = nil
        guard action.confirms else {
            flow = PinFlow(action)
            return
        }
        removal = (action, target)
    }

    /// The confirmed removal of a PIN the core takes without a keypad.
    private func removePin(_ action: PinAction, _ target: ProfileInfo) {
        busy = true
        Task {
            defer { busy = false }
            do {
                // No current PIN: the core takes that from the owner, and for one it cannot read.
                try await profiles.perform("setPin", ["profile": target.id, "pin": JSONValue.null])
                said = (action == .removeUnreadable ? "PIN removed" : "\(target.name)'s PIN was removed.", true)
                if action == .removeUnreadable { unreadable = nil }
            } catch {
                said = (problem(error), false)
            }
        }
    }

    /// One keypad entry in the PIN flow: the next step, or `setPin` once done.
    private func step(_ pin: String, _ target: ProfileInfo) async -> KeypadResult {
        guard var current = flow else { return .ok }
        guard let submit = current.enter(pin) else {
            flow = current
            return current.note.map { .problem($0) } ?? .ok
        }
        // The flow this save belongs to: its answer never closes or rewinds a newer one.
        let sent = current
        var fields: [String: any Sendable] = ["profile": target.id, "pin": submit.pin.map { JSONValue.string($0) } ?? .null]
        if let currentPin = submit.currentPin { fields["currentPin"] = currentPin }
        do {
            try await profiles.perform("setPin", fields)
            flow = PinFlow.settle(flow, sent: sent, ok: true, code: nil)
            // Said on the page whatever is on screen now: the save went through.
            said = (submit.pin == nil ? "PIN removed" : "PIN saved", true)
            return .ok
        } catch {
            let code = (error as? CoreError)?.code
            if code == "pinUnreadable" {
                // A PIN a newer version stored: the steps close and Remove is offered.
                if flow?.id == sent.id { flow = nil }
                unreadable = target.id
                return .ok
            }
            flow = PinFlow.settle(flow, sent: sent, ok: false, code: code)
            // The flow that asked is gone (the app went to the back, or a newer
            // one started): the page says how its save ended.
            if flow?.id != sent.id { said = (problem(error), false) }
            return .of(error)
        }
    }

    private func save(thenLeave: Bool = false) {
        let trimmed = name.trimmingCharacters(in: .whitespacesAndNewlines)
        guard (1...30).contains(trimmed.count) else {
            said = (ProfileRules.problem("badName", fallback: ""), false)
            return
        }
        busy = true
        Task {
            defer { busy = false }
            do {
                if let profileId {
                    var fields: [String: any Sendable] = ["profile": profileId, "name": trimmed]
                    if touched { fields["avatar"] = ProfileRules.formatAvatar(glyph: glyph, color: color) }
                    try await profiles.perform("updateProfile", fields)
                    touched = false
                    said = ("Saved", true)
                    if thenLeave { dismiss() }
                } else {
                    try await profiles.perform("createProfile", ["name": trimmed, "avatar": ProfileRules.formatAvatar(glyph: glyph, color: color)])
                    dismiss()
                }
            } catch {
                said = (problem(error), false)
            }
        }
    }

    private func delete(_ target: ProfileInfo) {
        // One delete at a time: a second press never sends another.
        guard !deleting else { return }
        deleting = true
        Task {
            do {
                try await profiles.perform("deleteProfile", ["profile": target.id])
                dismiss()
            } catch {
                deleting = false
                said = (problem(error), false)
            }
        }
    }

    private func problem(_ error: any Error) -> String {
        ProfileRules.problem((error as? CoreError)?.code, fallback: error.localizedDescription)
    }
}
