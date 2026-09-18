app-title = Top Bar Settings
search = Search settings
search-no-results = No settings match your search.
loading = Loading…
apply = Apply
cancel = Cancel
reset = Reset
restore = Restore
undo = Undo

page-top-bar = Top Bar
page-menu = System Menu
page-active-app = Active App
page-control-center = Control Center
page-windows = Windows & Touchpad

## Top Bar
profile = Profile
profile-name = macOS top bar
profile-applied = Applied
profile-not-applied = Not applied. Your panel still has COSMIC's layout.
profile-differs = { $count ->
    [one] One setting differs from the profile.
   *[other] { $count } settings differ from the profile.
}
applets-missing = These applets are not installed, so the panel cannot show them: { $applets }. Run `just install`.
result-applied = Applied. A backup was saved first.
result-nothing = Nothing to change.
result-restored = { $count ->
    [0] Restored. Nothing had changed.
    [one] Restored one setting.
   *[other] Restored { $count } settings.
}
appearance = Appearance
theme-preset = Theme Preset
theme-classic = Classic macOS
theme-liquid-glass = Liquid Glass (System-Wide)
opacity = Panel opacity
show-weekday = Show the weekday in the clock
keep-notifications = Keep the notifications applet
keep-notifications-description = Shows notification history next to the Control Center. Popups then appear next to it.
notifications = Notifications
notification-position = Popup position
notification-position-description = Where new notifications appear. Takes effect at once with the patched daemon below.
notification-position-applet = While the notifications applet is kept, popups appear next to it; this choice is used once the applet is removed.
position-top-left = Top left
position-top = Top center
position-top-right = Top right
position-bottom-left = Bottom left
position-bottom = Bottom center
position-bottom-right = Bottom right
position-left = Left
position-right = Right
notifications-daemon = Notification daemon
notifications-daemon-active = The patched daemon is running, so the position applies right away.
notifications-daemon-installed = The patched daemon is installed, but this session still runs COSMIC's. Log in again, or run `just restart-notifications` (the panel reloads too).
notifications-daemon-missing = COSMIC's daemon always shows popups at the top center when the notifications applet is not in the bar. A small patch teaches it this setting. Build and install it once from the project directory (asks for sudo), then log in again or restart it:
backups = Backups
undo-last = Undo the last change
undo-last-description = { $count ->
    [0] No backups yet.
    [one] Returns to the backup made before the last change.
   *[other] Returns to the latest of { $count } backups.
}
restore-original = Restore the original panel
restore-original-description = Back to the panel you had before this tool changed anything.
restore-original-confirm = Your panel, clock and session services return to how they were before the first change. The top bar applets stay installed.

## System Menu
menu-entries = Menu entries
show-about = About This Computer
show-app-store = App Store
show-app-store-description = Hidden automatically when COSMIC Store is not installed.
power = Power
confirm-power = Confirm restart, shut down and log out
confirm-power-description = Asks through COSMIC's dialog before acting.
menu-icon = Icon
icon-name = Icon name
icon-name-description = Any icon theme name, for example distributor-logo-archlinux.

## Active App
app-label = Label
bold = Bold name
max-chars = Longest name
max-chars-description = Longer names end with an ellipsis.
empty-label = Label with no window focused
empty-label-description = Leave empty for "Desktop". A single space hides the label.
desktop = Desktop
monitors = Multiple monitors
follow-output = Follow this monitor
follow-output-description = Each panel shows the app last used on its own monitor.
global-menu = Global menu
global-menu-toggle = Show app menus in the top bar
global-menu-description = Experimental. Works with Qt apps and some X11 apps. Restart apps after changing this.

## Control Center
cc-layout = Layout
cc-connectivity = Wi-Fi and Bluetooth
cc-toggles = Focus, dark mode and screenshot
cc-display = Display brightness
cc-sound = Sound
cc-shortcuts = Lock, settings and battery
cc-reset-description = Show every block in the default order.
cc-media = Media
now-playing = Now Playing
max-volume = Volume limit
max-volume-description = Highest volume the slider allows. Above 100% may distort.

## Windows & Touchpad
window-controls = Window controls
controls-left = Buttons on the left
controls-left-description = Close, minimize and maximize on the left in GTK apps, Firefox, Chromium and Telegram. COSMIC apps and COSMIC's title bars keep them on the right.
touchpad = Touchpad
three-finger-drag = Three-finger drag
three-finger-drag-description = Move three fingers to drag windows and select text, as on a Mac.
three-finger-drag-setup = Three-finger drag needs linux-3-finger-drag, installed once in a terminal with sudo. Then log in again.
copy-commands = Copy commands
project-page = Project page
