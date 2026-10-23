import Foundation

/// Uses the desktop Markdown boundaries; platform offsets remain UTF-16.
struct ComposerQuery: Equatable {
    let range: NSRange
    let query: String
    let skills: Bool

    static func active(in text: String, selection: NSRange) -> ComposerQuery? {
        guard selection.location >= 0, selection.length == 0, selection.location <= (text as NSString).length,
              let result = composerCompletionQuery(text: text, selectionStart: UInt32(selection.location), selectionEnd: UInt32(selection.location)) else { return nil }
        return ComposerQuery(range: NSRange(location: Int(result.start), length: Int(result.end - result.start)), query: result.query, skills: result.skills)
    }
}
