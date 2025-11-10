# Desain Alur Kerja: TUI Penerjemahan JSON

Dokumen ini menguraikan desain dan alur kerja untuk aplikasi TUI (Text-based
User Interface) yang dibangun menggunakan `ratatui` untuk memfasilitasi proses
penerjemahan string dari file JSON.

## 1. Tujuan

Menyediakan antarmuka yang cepat dan efisien bagi penerjemah untuk:

- Membandingkan file bahasa sumber dan target.
- Melihat daftar kunci (keys) beserta status terjemahannya.
- Memasukkan atau mengedit terjemahan dengan mudah.
- Menyimpan hasil ke dalam file JSON target tanpa merusak strukturnya.

## 2. Tampilan Utama (UI Layout)

Tampilan akan dibagi menjadi tiga panel utama untuk memisahkan konteks:

1.  **Panel Daftar Kunci (kiri):**
    - Sebuah daftar yang dapat di-scroll, berisi semua kunci dari file sumber.
    - Setiap item memiliki indikator status:
        - `[✓]` : Sudah diterjemahkan.
        - `[ ]` : Belum diterjemahkan.
    - Item yang aktif dipilih akan di-highlight.

2.  **Panel Teks Sumber (kanan atas):**
    - Menampilkan teks asli (dari bahasa sumber) untuk kunci yang sedang
      dipilih.
    - Panel ini bersifat *read-only* untuk referensi.

3.  **Panel Input/Edit (kanan bawah):**
    - Area untuk mengetik atau mengedit teks terjemahan.
    - Menampilkan terjemahan yang sudah ada jika kunci yang dipilih telah
      diterjemahkan.

## 3. Alur Interaksi (Modal Editing)

Aplikasi akan menggunakan dua mode utama untuk interaksi yang jelas dan
menghindari input yang tidak disengaja: **Mode Normal** dan **Mode Edit**.

### Mode Normal (Navigasi)

Ini adalah mode default saat aplikasi dimulai. Mode ini digunakan untuk
navigasi dan perintah umum.

- **Kontrol:**
    - `↑` / `↓` / `j` / `k`: Menavigasi daftar kunci.
    - `Enter`: Memilih kunci yang disorot dan beralih ke **Mode Edit**.
    - `Ctrl+S`: Menyimpan semua perubahan ke file JSON target.
    - `q`: Keluar dari aplikasi (dengan prompt konfirmasi jika ada perubahan
      belum disimpan).

### Mode Edit (Input Teks)

Mode ini aktif setelah menekan `Enter` pada sebuah kunci. Fokus berpindah ke
panel input, dan pengguna dapat mulai mengetik.

- **Kontrol:**
    - **Input Teks:** Semua ketikan standar (huruf, angka, simbol) akan
      dimasukkan sebagai teks terjemahan.
    - `Enter`: Menyimpan hasil editan untuk kunci saat ini (di dalam memori)
      dan kembali ke **Mode Normal**.
    - `Esc`: Membatalkan semua perubahan pada kunci saat ini dan kembali ke
      **Mode Normal**.

## 4. Skenario Penggunaan

1.  **Memulai Aplikasi:**
    - Pengguna menjalankan dari terminal: `penerjemah-tui source.json
      target.json`.
    - Aplikasi memuat file, membandingkan isinya, dan menampilkan TUI dalam
      **Mode Normal**.

2.  **Menerjemahkan Item Baru:**
    - Pengguna menavigasi ke kunci dengan status `[ ]`.
    - Menekan `Enter` untuk masuk **Mode Edit**.
    - Mengetik terjemahan di panel input.
    - Menekan `Enter` untuk konfirmasi. Status kunci berubah menjadi `[✓]` dan
      aplikasi kembali ke **Mode Normal**.

3.  **Menyimpan Pekerjaan:**
    - Setelah menerjemahkan beberapa item, pengguna menekan `Ctrl+S`.
    - Aplikasi menulis semua perubahan ke `target.json` dan menampilkan
      notifikasi singkat.

4.  **Keluar:**
    - Pengguna menekan `q`.
    - Jika ada perubahan yang belum disimpan, sebuah dialog akan muncul:
      `Simpan perubahan sebelum keluar? (y/n/c)`.

## 5. Tantangan Teknis

- **Widget Input Teks:** `ratatui` tidak memiliki widget input teks bawaan.
  Perlu implementasi custom untuk menangani kursor, input karakter, backspace,
  dll.
- **Integritas Data JSON:** Saat menyimpan, penting untuk hanya memperbarui
  nilai-nilai yang relevan tanpa mengubah urutan atau struktur file JSON asli,
  terutama jika mengandung *nested objects*. Library `serde_json` akan
  digunakan untuk tugas ini.
