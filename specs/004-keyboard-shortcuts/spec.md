# Feature Specification: Keyboard Shortcuts

**Feature Branch**: `004-keyboard-shortcuts`
**Created**: 2026-03-25
**Status**: Draft
**Input**: User description: "support des raccourcis clavier (ouvrir, save, save as, etc)"

## User Scenarios & Testing *(mandatory)*

### User Story 1 - Power User Operates Without the Mouse (Priority: P1)

A user working with many PDFs wants to open, review, and save files without reaching for the mouse. The most frequent actions — open, save, page navigation, zoom — must all be accessible from the keyboard.

**Why this priority**: These are the highest-frequency actions. Any user who spends more than a few minutes with the app will benefit immediately, and the shortcuts are expected by anyone familiar with standard desktop software.

**Independent Test**: Can be fully tested by opening the app, loading a PDF, and completing the full open → navigate → annotate → save workflow using only the keyboard.

**Acceptance Scenarios**:

1. **Given** the app is open, **When** the user presses Ctrl+O, **Then** the file picker opens.
2. **Given** a PDF is loaded, **When** the user presses Ctrl+S, **Then** the file is saved in place (same path) without prompting.
3. **Given** a PDF is loaded, **When** the user presses Ctrl+Shift+S, **Then** the Save As dialog opens.
4. **Given** a PDF is loaded, **When** the user presses Ctrl+P, **Then** the print action is triggered.
5. **Given** a PDF is loaded, **When** the user presses the Right arrow or Page Down, **Then** the next page is shown.
6. **Given** a PDF is loaded, **When** the user presses the Left arrow or Page Up, **Then** the previous page is shown.

---

### User Story 2 - User Controls Zoom From the Keyboard (Priority: P2)

A user reading a dense document wants to zoom in and out fluidly without clicking small buttons.

**Why this priority**: Zoom is used constantly while reading. Ctrl++ / Ctrl+- are universally expected from browsers and document viewers.

**Independent Test**: Can be tested independently by loading a PDF and verifying zoom changes via keyboard alone, including reset to 100%.

**Acceptance Scenarios**:

1. **Given** a PDF is loaded, **When** the user presses Ctrl++ (or Ctrl+=), **Then** the zoom increases by one snap level.
2. **Given** a PDF is loaded, **When** the user presses Ctrl+- , **Then** the zoom decreases by one snap level.
3. **Given** a PDF is loaded at any zoom, **When** the user presses Ctrl+0, **Then** the zoom resets to 100%.

---

### User Story 3 - User Triggers Compress From the Keyboard (Priority: P3)

A user who regularly compresses PDFs wants to open the compress modal without moving to the toolbar.

**Why this priority**: Less frequent than save/zoom, but consistent with the principle that all major toolbar actions should have a keyboard equivalent.

**Independent Test**: Can be tested by pressing the shortcut and verifying the compress modal appears.

**Acceptance Scenarios**:

1. **Given** a PDF is loaded, **When** the user presses Ctrl+Shift+E, **Then** the compression modal opens.

---

### Edge Cases

- Shortcuts must not fire when the user is typing in a text input (page number field, zoom field, annotation text box) — standard browser focus rules apply.
- If no PDF is loaded, shortcuts that require an open document (Save, Save As, Compress, navigation, zoom) must be silently ignored.
- Ctrl+S on an unmodified document (nothing changed since last save) should be a no-op or confirm silently — it must never show an error.
- Conflicting shortcuts with the OS or browser layer (e.g., Ctrl+W closes browser tabs) must be avoided.

## Requirements *(mandatory)*

### Functional Requirements

- **FR-001**: Pressing Ctrl+O MUST open the file picker, regardless of whether a document is currently open.
- **FR-002**: Pressing Ctrl+S MUST save the current document to its existing path without any dialog, when a document is open.
- **FR-003**: Pressing Ctrl+Shift+S MUST open the Save As dialog, when a document is open.
- **FR-004**: Pressing Ctrl+P MUST trigger the print action, when a document is open.
- **FR-005**: Pressing Ctrl++ or Ctrl+= MUST increase the zoom by one snap level, when a document is open.
- **FR-006**: Pressing Ctrl+- MUST decrease the zoom by one snap level, when a document is open.
- **FR-007**: Pressing Ctrl+0 MUST reset zoom to 100%, when a document is open.
- **FR-008**: Pressing the Right arrow or Page Down key MUST navigate to the next page, when a document is open and no text input is focused.
- **FR-009**: Pressing the Left arrow or Page Up key MUST navigate to the previous page, when a document is open and no text input is focused.
- **FR-010**: Pressing Ctrl+Shift+E MUST open the compress modal, when a document is open.
- **FR-011**: All shortcuts MUST be suppressed when a text input field (page number, zoom, annotation) has keyboard focus.
- **FR-012**: All document-requiring shortcuts MUST be silently ignored when no document is loaded.

## Success Criteria *(mandatory)*

### Measurable Outcomes

- **SC-001**: All 10 shortcuts listed in FR-001 through FR-010 work correctly in a single session without mouse interaction.
- **SC-002**: No shortcut fires unintentionally while the user is typing in any input field within the app.
- **SC-003**: A user familiar with standard document viewer shortcuts (Acrobat, browser PDF viewer) can operate the app without consulting any documentation.

## Assumptions

- Arrow key navigation (FR-008, FR-009) only fires when no focusable element (input, button) has focus — standard browser keyboard event bubbling behavior applies.
- Ctrl+Shift+E was chosen for compress as it avoids conflicts with common OS/browser shortcuts. This can be revised.
- "Save" (Ctrl+S) saves in-place to the original file path; if the file was opened but never saved (no path), it falls back to Save As behavior.
- Zoom snap levels are the same as the existing snap-to-level behavior already implemented in the toolbar.
- No new UI is needed (no shortcut reference panel required for this iteration).
