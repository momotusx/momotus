import XCTest

extension XCUIApplication {
    /// First run's quickest way past (plans/first-screens.md 2): Get started, Not now (with a sync server), Continue on
    /// the profile form, Skip on the setup sheet, which adds the recommended sources and opens Home. False when the app
    /// did not open on Welcome.
    @discardableResult
    func passFirstRun(timeout: TimeInterval) -> Bool {
        let start = buttons["Get started"]
        guard start.waitForExistence(timeout: timeout) else { return false }
        start.tap()
        for title in ["Not now", "Continue", "Skip"] {
            let button = buttons[title].firstMatch
            if button.waitForExistence(timeout: 10) { button.tap() }
        }
        // Skip adds the recommended sources, then Home opens; offline it stays on the sheet.
        return buttons["Skip"].firstMatch.waitForNonExistence(timeout: 20)
    }
}
