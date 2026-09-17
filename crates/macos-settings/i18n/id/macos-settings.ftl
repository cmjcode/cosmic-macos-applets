app-title = Pengaturan Top Bar
search = Cari pengaturan
search-no-results = Tidak ada pengaturan yang cocok.
loading = Memuat…
apply = Terapkan
cancel = Batal
reset = Atur ulang
restore = Pulihkan
undo = Urungkan

page-top-bar = Top Bar
page-menu = Menu Sistem
page-active-app = Aplikasi Aktif
page-control-center = Control Center
page-windows = Jendela & Touchpad

## Top Bar
profile = Profil
profile-name = Top bar macOS
profile-applied = Sudah diterapkan
profile-not-applied = Belum diterapkan. Panel masih memakai tata letak COSMIC.
profile-differs = { $count ->
    [one] Satu pengaturan berbeda dari profil.
   *[other] { $count } pengaturan berbeda dari profil.
}
applets-missing = Applet ini belum terpasang, jadi panel tidak bisa menampilkannya: { $applets }. Jalankan `just install`.
result-applied = Diterapkan. Backup sudah disimpan lebih dulu.
result-nothing = Tidak ada yang perlu diubah.
result-restored = { $count ->
    [0] Dipulihkan. Tidak ada yang berubah.
    [one] Satu pengaturan dipulihkan.
   *[other] { $count } pengaturan dipulihkan.
}
appearance = Tampilan
opacity = Opasitas panel
show-weekday = Tampilkan nama hari di jam
keep-notifications = Pertahankan applet notifikasi
keep-notifications-description = Menampilkan riwayat notifikasi di samping Control Center. Popup lalu muncul di sebelahnya.
notifications = Notifikasi
notification-position = Posisi popup
notification-position-description = Tempat notifikasi baru muncul. Langsung berlaku dengan daemon yang sudah dipatch di bawah.
notification-position-applet = Selama applet notifikasi dipertahankan, popup muncul di sebelahnya; pilihan ini dipakai setelah applet dilepas.
position-top-left = Kiri atas
position-top = Tengah atas
position-top-right = Kanan atas
position-bottom-left = Kiri bawah
position-bottom = Tengah bawah
position-bottom-right = Kanan bawah
position-left = Kiri
position-right = Kanan
notifications-daemon = Daemon notifikasi
notifications-daemon-active = Daemon yang dipatch sedang berjalan, jadi posisi langsung berlaku.
notifications-daemon-installed = Daemon yang dipatch sudah terpasang, tetapi sesi ini masih menjalankan daemon bawaan COSMIC. Masuk lagi, atau jalankan `just restart-notifications` (panel ikut dimuat ulang).
notifications-daemon-missing = Daemon COSMIC selalu menampilkan popup di tengah atas saat applet notifikasi tidak ada di bar. Patch kecil membuatnya membaca pengaturan ini. Bangun dan pasang sekali dari direktori proyek (meminta sudo), lalu masuk lagi atau mulai ulang daemonnya:
backups = Backup
undo-last = Urungkan perubahan terakhir
undo-last-description = { $count ->
    [0] Belum ada backup.
    [one] Kembali ke backup sebelum perubahan terakhir.
   *[other] Kembali ke backup terbaru dari { $count } backup.
}
restore-original = Pulihkan panel asli
restore-original-description = Kembali ke panel sebelum alat ini mengubah apa pun.
restore-original-confirm = Panel, jam, dan layanan sesi kembali seperti sebelum perubahan pertama. Applet top bar tetap terpasang.

## System Menu
menu-entries = Isi menu
show-about = Tentang Komputer Ini
show-app-store = App Store
show-app-store-description = Otomatis disembunyikan kalau COSMIC Store tidak terpasang.
power = Daya
confirm-power = Konfirmasi mulai ulang, matikan, dan keluar
confirm-power-description = Bertanya lewat dialog COSMIC sebelum dijalankan.
menu-icon = Ikon
icon-name = Nama ikon
icon-name-description = Nama ikon apa pun dari tema ikon, misalnya distributor-logo-archlinux.

## Active App
app-label = Label
bold = Nama tebal
max-chars = Panjang nama maksimal
max-chars-description = Nama yang lebih panjang diakhiri elipsis.
empty-label = Label saat tidak ada jendela aktif
empty-label-description = Kosongkan untuk "Desktop". Satu spasi menyembunyikan label.
desktop = Desktop
monitors = Beberapa monitor
follow-output = Ikuti monitor ini
follow-output-description = Tiap panel menampilkan aplikasi terakhir di monitornya sendiri.
global-menu = Menu global
global-menu-toggle = Tampilkan menu aplikasi di top bar
global-menu-description = Eksperimental. Berfungsi untuk aplikasi Qt dan sebagian aplikasi X11. Mulai ulang aplikasi setelah mengubahnya.

## Control Center
cc-layout = Tata letak
cc-connectivity = Wi-Fi dan Bluetooth
cc-toggles = Fokus, mode gelap, dan tangkapan layar
cc-display = Kecerahan layar
cc-sound = Suara
cc-shortcuts = Kunci, pengaturan, dan baterai
cc-reset-description = Tampilkan semua blok dengan urutan bawaan.
cc-media = Media
now-playing = Sedang Diputar
max-volume = Batas volume
max-volume-description = Volume tertinggi yang diizinkan slider. Di atas 100% bisa pecah.

## Windows & Touchpad
window-controls = Tombol jendela
controls-left = Tombol di kiri
controls-left-description = Tutup, minimalkan, dan maksimalkan di kiri untuk aplikasi GTK, Firefox, Chromium, dan Telegram. Aplikasi COSMIC dan title bar COSMIC tetap di kanan.
touchpad = Touchpad
three-finger-drag = Drag tiga jari
three-finger-drag-description = Geser dengan tiga jari untuk memindahkan jendela dan memilih teks, seperti di Mac.
three-finger-drag-setup = Drag tiga jari butuh linux-3-finger-drag, dipasang sekali lewat terminal dengan sudo. Setelah itu login ulang.
copy-commands = Salin perintah
project-page = Halaman proyek
