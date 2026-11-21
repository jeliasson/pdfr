# Why national eID / certificate PDF signing apps get low ratings

Scope note: about 16 tool calls. Sources are GitHub issue trackers, store pages (Google Play and the Apple App Store, fetched through a rendering proxy), the Adobe community, Linux distro forums and German tech blogs. **Reddit could not be fetched** (the proxy fetch was refused by the permission system, and web search returned no Reddit threads). Flathub returned 503. Searches for Italian apps (ArubaSign, Dike GoSign, CIE) returned nothing usable. Counts below are only what the pages showed. Nothing here is a statistically sampled frequency.

## Top reasons for 1–2 star reviews, and their rough frequency

### Takeaway
Across the sources, complaints fall into a few recurring groups:
- The local helper (AutoFirma) cannot be reached from the browser.
- No certificates appear: keychain or certificate store problems, missing root CAs.
- The card or NFC is not read, which in some cases also causes false PIN-lockout messages.
- The Java runtime breaks.
- Signatures are rejected in Adobe Acrobat ("Unsupported algorithm").
- The mobile app is a stub that doesn't do what the desktop app does.
- Errors are cryptic and logs are empty.

"Works on Windows, broken on Mac/Linux" runs through all of these.

### Cited findings

**Spain: AutoFirma / Cliente @firma**
- The AutoFirma iOS app ("Autofirma App", formerly "Cliente @firma móvil") is rated **1.3/5 from about 2,000 ratings** on the Spanish App Store. — [App Store ES reviews](https://apps.apple.com/es/app/cliente-firma-movil/id627410001?see-all=reviews)
  - "La mitad de las veces no funciona sin saberse por qué." (Translation: "Half the time it doesn't work and you can't tell why.") — Remate62, 04/05/2024, same URL
  - "El ministerio dice que está obsoleta y que recomienda usar Autofirma, pero no facilita la forma de descargar Autofirma en iOS" (Translation: "The ministry says it's obsolete and recommends Autofirma, but gives no way to get Autofirma on iOS.") — LuixLegan, 08/05/2023
  - Older reviews (2021, labelled old): "No sirve para nada tenerlo en el teléfono... no lo reconocen los portales oficiales por estar desfasado y remitirte a AutoFirma para ordenador" (Translation: "Useless on the phone... official portals don't accept it because it's outdated and send you to desktop AutoFirma."). Also: "la App no funciona y muestra un error de sintaxis" (Translation: "the app doesn't work and shows a syntax error."). — same URL
  - A recent positive review (17 Sept, year not shown, likely 2026) also names a gap: "No me ha cogido automáticamente los certificados instalados en el movil" (Translation: "It didn't pick up the certificates installed on the phone automatically.").
- **"No se pudo contactar con AutoFirma"** ("could not contact AutoFirma") is the main desktop failure. In it, the browser launches AutoFirma, but AutoFirma never starts its local WebSocket server. [Issue #416](https://github.com/ctt-gob-es/clienteafirma/issues/416), opened 3 Oct 2024 and still open with comments into Feb 2026, reports it on macOS Sequoia across Safari, Chrome, Firefox, Edge, Opera and Arc. Quotes from the thread:
  - "El problema persiste, tanto con versiones antiguas de AutoFirma, como con la última" (Translation: "The problem persists with old versions and the latest.")
  - "cuando el navegador abre autofirma, éste no logra abrirse bien y montar el servidor de websockets" (Translation: "when the browser opens autofirma, it fails to open properly and start the websocket server") — 7 Oct 2024
  - "Yo al final tuve que usar el portátil windows de un amigo... Imposible hacerlo desde Mac" (Translation: "In the end I had to use a friend's Windows laptop... impossible from a Mac") — 30 Nov 2024
  - "me pide que instale Rosetta, aún cuando la versión que me he bajado es la de M1 y M2" (Translation: "it asks me to install Rosetta even though I downloaded the M1/M2 build")
  - "Los logs no muestran nada." (Translation: "The logs show nothing.") — 16 Dec 2024
  - "Me parece alucinante que con mi Mac viejo funcione y con el nuevo no." (Translation: "Incredible that it works on my old Mac and not the new one.") — 21 Feb 2026
  - Workarounds are inconsistent: "Restaurar instalación" (restore installation), moving the certificates from the login keychain to the System keychain, staying on version 1.8.2.
- The same error appears on other platforms in issues [#250](https://github.com/ctt-gob-es/clienteafirma/issues/250) and [#167](https://github.com/ctt-gob-es/clienteafirma/issues/167) ("Autofirma no carga para firmar desde el navegador", i.e. Autofirma doesn't load to sign from the browser). I saw only the titles of these.
- **Certificates not found on macOS** ([issue #520](https://github.com/ctt-gob-es/clienteafirma/issues/520), opened 22 May 2026, 4 participants, open):
  - "AutoFirma does not detect any valid certificate from macOS Keychain" (`CertificatesNotFoundException`) on AutoFirma 1.9.2, macOS Tahoe 26.4.1, Apple Silicon.
  - Root cause found by a user: "the empty dialog is caused by 'Always Trust' on the personal certificate... Setting it back to Always Trust makes the certificate disappear again" (pablopen, 20 Oct 2026). His explanation, that Java's KeychainStore filters out certificates with trust overrides, is an unverified hypothesis.
  - No maintainer reply is visible.
- **"La entrada no tiene clave privada"** ("the entry has no private key"): AutoFirma 1.9.2 on macOS throws this in AppleKeyStoreManager for an FNMT certificate installed through the official FNMT app. Re-importing doesn't fix it. — [issue #557](https://github.com/ctt-gob-es/clienteafirma/issues/557) (title only seen)
- **Java dependency:**
  - The official install manual warns that the GUI may not open on Linux because "muchas distribuciones de Linux incluyen por defecto una versión headless de la JRE" (Translation: "many Linux distributions ship a headless JRE by default").
  - The manual also describes an RPM "NOKEY" error, and an uninstaller bug in versions 1.7.1 and earlier that broke upgrades. — [AutoFirma manual de instalación v1.9](https://firmaelectronica.gob.es/content/dam/firmaelectronica/ciudadanos/descargas/pdf/AF-manual-instalacion-usuarios-ES%20v1-9.pdf)
  - On Linux Mint, AutoFirma 1.8.3 rejected Java 21.0.7 and blocked an FNMT certificate renewal. — [Linux Mint forum](https://forums.linuxmint.com/viewtopic.php?t=447388)
- **The cryptic SAF_xx error codes** are common enough to support third-party explainer sites:
  - For SAF_15, the suggested fix is to disable "Habilitar JMulticard para el uso de las tarjetas de la FNMT y DNIe" (the option to use JMulticard for FNMT cards and DNIe). — [info-autofirma.es](https://info-autofirma.es/errores-frecuentes/)
  - SAF_45 on macOS is traced to ~/Library/Application Support/AutoFirma being owned by root. This comes from a third-party blog and is unverified. — [macdnie.com](https://macdnie.com/en/autofirma-mac.html)
  - The same explainer site's advice: "si usas Mac o Linux y el trámite sigue dando guerra, prueba desde Windows" (Translation: "if you're on Mac or Linux and it still fights you, try Windows"). — [info-autofirma.es](https://info-autofirma.es/errores-frecuentes/)
- AutoFirma does not install the needed FNMT root certificates by itself. Missing roots lead to the "no certificates shown" failure. — [jasoft.org](https://www.jasoft.org/Blog/post/solucion-a-que-autofirma-no-te-muestre-ningun-certificado-digital)
- There is a commercial market for working around this. "DNIe para Mac" is a paid app with a 3-day trial, and its developer comments in AutoFirma issues. — [issue #520](https://github.com/ctt-gob-es/clienteafirma/issues/520)
- FNMT certificates also fail when signing inside Adobe Acrobat (2022, older), even for users whose AutoFirma works. — [Adobe community](https://community.adobe.com/t5/acrobat-reader-discussions/error-signing-documents-with-certificate/td-p/13251202), [Adobe community, FNMT](https://community.adobe.com/questions-9/error-al-firmar-con-certificado-electronico-de-la-fnmt-1264102)

**Germany: AusweisApp (identification, not PDF signing, but uses the same card and NFC)**
- Google Play: **4.5 stars, about 76.4K reviews, 10M+ downloads**, updated 16 Nov 2026. — [Google Play](https://play.google.com/store/apps/details?id=com.governikus.ausweisapp2&hl=en&gl=de)
- 1-star themes on that page:
  - The NFC read fails and the app then shows a false wrong-PIN lockout: "it locks someone out and claims an id was entered improperly 3 times not because this happened but because the card wasn't picked up or scanned by the phone" (A Frank, 21 Oct 2026)
  - "just another German app that doesn't work... And the help desk closes at 5pm!" (16 Nov 2026)
  - "Useless and doesn't work" (9 Nov 2026)
- Using an iPhone as a card reader for a Mac or iPad stopped pairing reliably after version 2.6.2 (Oct/Nov 2026): "Betroffene berichten, dass sich die Geräte teilweise überhaupt nicht mehr koppeln lassen" (Translation: "those affected report that the devices sometimes can't be paired at all"). — [appgefahren.de](https://www.appgefahren.de/ausweisapp-2-6-2-sicherheitsluecken-geschlossen-iphone-verbindung-macht-probleme-407621.html)
- Older Android reports:
  - "Schwacher NFC Empfang" (weak NFC reception) on the Pixel 5, with 8 of 14 poll respondents unresolved. — [android-hilfe.de](https://www.android-hilfe.de/forum/google-pixel-5.3794/ausweisapp2-meldet-schwacher-nfc-empfang.972078.html)
  - "Extended length" errors on Samsung S10 after Android 12, later fixed by a Samsung update. — [Samsung community](https://eu.community.samsung.com/t5/andere-galaxy-s-serien/extended-length-fehler-bei-nfc-z-b-ausweisapp2-nach-android-12/td-p/4577986/page/2)
- Trustpilot has 42 reviews, mostly negative, about failed NFC connections and crashes. — [Trustpilot](https://de.trustpilot.com/review/www.ausweisapp.bund.de?page=2)

**Belgium: eID middleware / eID Viewer**
- **Acrobat says "Unsupported algorithm" with the Belgian eID**:
  - First reported Jan 2022 after a new card was issued. The accepted fix was to unlink the PKCS#11 module in Acrobat.
  - But on macOS 15.6 (Aug 2025), a user found that after removing the PKCS#11 module "there is no certificate shown to use to sign".
  - A Romanian CEI user in the same thread got "Adobe Acrobat could not find any new digital IDs".
  - Adobe's Sep 2025 reply: update Acrobat, unlink PKCS#11, update Trust Manager.
  - Sources: [Adobe community thread](https://community.adobe.com/t5/acrobat-reader-discussions/niet-ondersteund-algoritme-unsupported-algoritme/m-p/15500537/highlight/true), [another reply](https://community.adobe.com/t5/acrobat-reader-discussions/niet-ondersteund-algoritme-unsupported-algoritme/m-p/15588591/highlight/true)
- Card reader not detected on Linux:
  - On Linux Mint the fix was installing pcscd. — [Mint forum](https://forums.linuxmint.com/viewtopic.php?t=222225)
  - On openSUSE, pcscd must be enabled by hand, per the official FAQ. — [eid.belgium.be](https://eid.belgium.be/en/faq/opensuse-my-card-reader-not-found-what-should-i-do)
  - On Manjaro, an ACS reader listed as supported didn't work while a Sitecom reader did. — [Manjaro forum](https://forum.manjaro.org/t/belgian-eid-not-working/103106)
  - On MX Linux, the reader got no USB power. — [MX forum](https://forum.mxlinux.org/viewtopic.php?t=61180)
- A third-party Mac app, "eidReader", sells itself on reading the Belgian eID "without additional drivers or Java" and on fixing one specific reader (Vasco Digipass 870). That points to middleware pain on macOS. — [App Store](https://apps.apple.com/ca/app/eidreader/id1190651975)

**Estonia: DigiDoc4**
- Older Linux issues (2019) show the card being read while the user interface says otherwise:
  - [#507](https://github.com/open-eid/DigiDoc4-Client/issues/507): "Card is not inserted correctly" on Linux, with a new card while an old card worked.
  - [#583](https://github.com/open-eid/DigiDoc4-Client/issues/583): Fedora 30, "no card in reader" even though the personal ID code was read.
  - [#190](https://github.com/open-eid/DigiDoc4-Client/issues/190): Debian Sid, "rakendus ei tuvasta kaarti mittetoetatud platvormil" (Translation: "the app doesn't detect the card on an unsupported platform").
- Recent: [#1417](https://github.com/open-eid/DigiDoc4-Client/issues/1417) (June 2026). Smart-ID signing doesn't work in the Fedora-packaged qdigidoc 4.10.0. The issue is linked to Flathub and Red Hat Bugzilla bugs.
- Distro coverage: "Please support Debian GNU/Linux". — [open-eid/linux-installer #16](https://github.com/open-eid/linux-installer/issues/16)
- Foreign eIDAS cards are not supported. — [#872](https://github.com/open-eid/DigiDoc4-Client/issues/872) "Are there any plans to support other eIDAS cards?"
- Architecture: on Linux and macOS, DigiDoc4 uses **OpenSC PKCS#11** as the default signing driver. — [open-eid.github.io](https://open-eid.github.io/)

**Portugal: Autenticação.gov** (GitHub issues, Sep 2025 – Nov 2026). [Issue list](https://github.com/amagovpt/autenticacao.gov/issues):
- macOS crashes:
  - #250: crash when signing a PDF with Chave Móvel Digital on macOS 27.0.1, Apple Silicon (pteiddialogsQTsrv SIGSEGV). Sep 2026, 7 comments.
  - #253: the PIN dialog doesn't appear, macOS 27, Apple Silicon. Oct 2026, 9 comments.
  - #246: crash on "Reenviar SMS" (resend SMS).
- No native Apple Silicon build: #229 (Dec 2025, 9 comments) and #241 (May 2026).
- Linux:
  - #263: in the Flatpak 3.16.0, the PIN dialog segfaults in fontconfig when the host has WOFF fonts. Nov 2026.
  - #244: request for a Flatpak repository.
  - #210: a community tutorial on installing under Debian 11/12 via Flatpak, with 18 comments.
  - #227: Arch Linux reader compatibility.
- Card read errors: #259 "Erro 257 ao ler o cartão" (error 257 reading the card), #228 PTEID_ExParamRange on IAS5 cards, #218 EIDMW_ERR_PARAM_BAD.
- Signing UX requests: #217 different signature positions in batch signing, #212 caching the signing PIN per session, #260 signing encrypted PDFs that open without a password.
- The official manual has troubleshooting entries for "6.2 Não é possível adicionar o módulo PKCS#11 ao Adobe Acrobat Reader em MacOS" (cannot add the PKCS#11 module to Adobe Acrobat Reader on macOS) and "reader installed but not detected". — [Manual v3](https://amagovpt.github.io/docs.autenticacao.gov/Manual_de_Utilizacao_v3.pdf)

### Inferences
Ranked by how often each theme came up across sources. This is qualitative, not counted.
1. The local helper or browser bridge breaks (AutoFirma).
2. No certificate or card is visible: keychain trust, root CAs, pcscd, or the card isn't picked up.
3. Platform crashes on macOS updates and Apple Silicon (AutoFirma, Autenticação.gov).
4. Java or runtime packaging (AutoFirma).
5. Acrobat interop: "Unsupported algorithm", PKCS#11 linking.
6. Mobile apps that are stubs (AutoFirma iOS).
7. Cryptic codes (SAF_xx, EIDMW_ERR_*, error 257) and empty logs.
8. Signing UX gaps: placement, batch, PIN caching.

The AusweisApp's high average (4.5) shows that card and NFC apps can be rated well. The worst ratings (AutoFirma iOS, 1.3) go to apps that fail to do the core job at all.

### Gaps
- No Reddit or forocoches material (fetch blocked). No Microsoft Store or Flathub ratings (Flathub returned 503).
- No store ratings found for the Belgian eID apps, RIA DigiDoc mobile (the Play page wouldn't render through WebFetch), Autenticação.gov mobile (wrong package id, 404), the FNMT apps, ArubaSign, Dike GoSign or the CIE apps (the GoSign App Store URL returned 404).
- No reliable per-theme percentages. Frequencies above are qualitative.
- Timestamp/TSA complaints and "no visible signature placement" complaints were not specifically found, apart from Portugal #217 on batch positions.

## What specifically fails on Linux and macOS

### Takeaway
- **macOS:** the trouble is keychain trust and certificate-store handling, missing System-keychain roots, the Rosetta and Apple Silicon split, a broken localhost WebSocket bridge, and crashes in Qt PIN dialogs after OS updates.
- **Linux:** the trouble is pcscd not installed or not enabled, headless or wrong Java, distro packaging gaps (Debian, Arch, RPM keys), Flatpak sandbox and font crashes, and the card not being recognised although the reader sees it.

### Cited findings
**macOS**
- The AutoFirma WebSocket helper hangs. Old Macs work and new ones don't. Certificates sometimes have to be copied to the System keychain. — [#416](https://github.com/ctt-gob-es/clienteafirma/issues/416)
- A personal certificate marked "Always Trust" disappears from AutoFirma's picker. — [#520](https://github.com/ctt-gob-es/clienteafirma/issues/520)
- AutoFirma fails with a no-private-key error in the Apple keystore manager. — [#557](https://github.com/ctt-gob-es/clienteafirma/issues/557)
- Separate AutoFirma builds are needed for Intel, Apple Silicon and specific macOS versions (a 1.9.2 build specifically for Tahoe 26.4). — [WebSearch summary of macdnie.com / official site; secondary](https://macdnie.com/en/autofirma-mac.html)
- Autenticação.gov's PIN dialog crashes or doesn't appear on macOS 27 on Apple Silicon, and there is no native ARM build. — [amagovpt issues](https://github.com/amagovpt/autenticacao.gov/issues)
- Acrobat on macOS shows no certificate after the PKCS#11 module is unlinked (Belgian eID, macOS 15.6, Aug 2025). — [Adobe community](https://community.adobe.com/t5/acrobat-reader-discussions/niet-ondersteund-algoritme-unsupported-algoritme/m-p/15500537/highlight/true)
- Using an iPhone as an NFC reader for a Mac with AusweisApp 2.6.2 stopped pairing reliably. — [appgefahren.de](https://www.appgefahren.de/ausweisapp-2-6-2-sicherheitsluecken-geschlossen-iphone-verbindung-macht-probleme-407621.html)

**Linux**
- Belgian eID readers aren't seen until pcscd is installed or enabled, and some readers listed as supported don't work. — [Mint](https://forums.linuxmint.com/viewtopic.php?t=222225), [openSUSE FAQ](https://eid.belgium.be/en/faq/opensuse-my-card-reader-not-found-what-should-i-do), [Manjaro](https://forum.manjaro.org/t/belgian-eid-not-working/103106)
- AutoFirma: headless JRE, RPM NOKEY, Java 21 rejected. — [manual](https://firmaelectronica.gob.es/content/dam/firmaelectronica/ciudadanos/descargas/pdf/AF-manual-instalacion-usuarios-ES%20v1-9.pdf), [Mint](https://forums.linuxmint.com/viewtopic.php?t=447388)
- DigiDoc4 says "no card in reader" although the reader and card are read, and Debian lacks support. — [#583](https://github.com/open-eid/DigiDoc4-Client/issues/583), [#190](https://github.com/open-eid/DigiDoc4-Client/issues/190), [linux-installer #16](https://github.com/open-eid/linux-installer/issues/16)
- The Autenticação.gov Flatpak PIN dialog segfaults in fontconfig, and Debian and Arch need community tutorials. — [amagovpt issues](https://github.com/amagovpt/autenticacao.gov/issues)

### Inferences
- On Linux the failures are mostly about the plumbing: pcscd, packaging, Java.
- On macOS they are mostly about certificate stores and trust, plus regressions with each OS release that vendors are slow to fix (issue #416 has been open for 2 years).

### Gaps
- Not verified: Snap or Flathub user ratings, and browser-extension breakage specifics (for example the Belgian eID Firefox/Chrome extension, or the Web eID extension) in 2024–2026.

## What a third-party PKCS#11 / p11-kit / NSS signer can and can't solve

### Takeaway
A native PDF signer can remove the most common failure layers:
- the localhost helper and browser bridge;
- the Java runtime;
- certificate-store selection, by reading PKCS#11 tokens through p11-kit (OpenSC, beidpkcs11, pteidpkcs11), soft certificates from NSS (Firefox/Chrome) or PKCS#12 files, and on macOS the keychain;
- Acrobat interop (it signs directly);
- visible placement, batch signing and PIN-per-session UX;
- readable error messages.

It cannot fix:
- reader hardware, power or firmware;
- pcscd not running (though it can detect this and explain it);
- vendor middleware bugs, or cards whose PKCS#11 module is missing;
- NFC antenna problems on phones;
- government portals that require their own helper protocol (AutoFirma's `afirma://` and WebSocket flow);
- validation policies of the receiving party.

### Cited findings
- The DigiDoc4 architecture already uses the OpenSC PKCS#11 module for signing on Linux and macOS. This shows the national card can be driven through standard PKCS#11 without the government GUI. — [open-eid.github.io](https://open-eid.github.io/)
- The Belgian eID worked in Acrobat once Acrobat's own PKCS#11 linking was removed. This shows the failures are often in the app's integration layer rather than in the card. — [Adobe community](https://community.adobe.com/t5/acrobat-reader-discussions/niet-ondersteund-algoritme-unsupported-algoritme/m-p/15500537/highlight/true)
- Several AutoFirma failures sit in its own Java KeychainStore and localhost bridge (#416, #520, #557), not in the certificate. — links above
- Hardware and daemon-level failures sit outside any app: reader not powered (MX Linux), pcscd missing, phone NFC antenna or OS bugs. — [MX forum](https://forum.mxlinux.org/viewtopic.php?t=61180), [Mint](https://forums.linuxmint.com/viewtopic.php?t=222225), [Samsung community](https://eu.community.samsung.com/t5/andere-galaxy-s-serien/extended-length-fehler-bei-nfc-z-b-ausweisapp2-nach-android-12/td-p/4577986/page/2)
- In Autenticação.gov, Chave Móvel Digital (cloud signature by SMS OTP) crashes are app-side (#246, #250). A third party would need the CMD remote-signing API to support CMD at all. — [amagovpt issues](https://github.com/amagovpt/autenticacao.gov/issues)

### Inferences
**Solvable, with a big win:**
- No Java and no localhost helper.
- A unified certificate picker across p11-kit modules, NSS and PKCS#12. Do not filter out certificates because of trust overrides (the AutoFirma #520 trap).
- Auto-detect and explain: "pcscd not running", "no PKCS#11 module for this card; install beid/opensc/pteid", "card present but locked".
- PIN retry counter shown before entry, so a failed read can't be mistaken for a wrong PIN (the AusweisApp complaint).
- PAdES-B-LTA output that Acrobat validates: AIA/OCSP embedding and a TSA timestamp.
- Visible signature placement and batch signing.

**Partly solvable:** a missing vendor PKCS#11 module could be bundled or pointed to (OpenSC covers the Estonian and many other cards; Belgian beidpkcs11 and Portuguese pteidpkcs11 are separate).

**Not solvable:** reader hardware, NFC, card-issuance problems, government portals that require AutoFirma or Cl@ve flows, and remote/cloud signatures (CMD, Cl@ve Firma, Smart-ID) without their APIs.

### Gaps
- Not checked whether beidpkcs11 or pteidpkcs11 register with p11-kit by default on major distros.
- Not checked which national cards OpenSC supports today.

## Usage data for context

### Takeaway
Only partial numbers were obtained.

### Cited findings
- AusweisApp Bund (Android): 10M+ downloads, about 76.4K reviews, 4.5 stars (Nov 2026). — [Google Play](https://play.google.com/store/apps/details?id=com.governikus.ausweisapp2&hl=en&gl=de)
- AutoFirma iOS app: about 2,000 ratings at 1.3/5 (Spanish store). — [App Store ES](https://apps.apple.com/es/app/cliente-firma-movil/id627410001?see-all=reviews)
- Autenticação.gov GitHub: the Debian Flatpak tutorial issue has 18 comments, and macOS issues have 5–9 comments each. This is weak evidence of a small but active Linux and Mac user base. — [amagovpt issues](https://github.com/amagovpt/autenticacao.gov/issues)

### Gaps
- No download or user numbers found for AutoFirma desktop, FNMT, Belgian eID, DigiDoc4 (Flathub install stats unavailable), Autenticação.gov, or the Italian apps (CIE, ArubaSign, GoSign).
- National user-count statistics (for example, the number of active eID signers per country) were not researched in this pass.
