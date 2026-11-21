# Why users give 1–2 star ratings to mainstream commercial PDF apps (Acrobat, Foxit, PDF Expert, PDFelement, Xodo, PDF-XChange)

**How the reviews were gathered (read this first).** Store web pages show only a few reviews and no star ratings, so I pulled starred reviews straight from the stores:
- **Apple App Store (iOS + Mac):** Apple's public customer-review RSS feed, `https://itunes.apple.com/us/rss/customerreviews/page=N/id=<APPID>/sortby=mostrecent/json`, pages 1–10 (at most 500 most recent US reviews per app).
- **Google Play:** the open-source `google-play-scraper` library, pulling the newest 200 one-star and 200 two-star US reviews per app.
- **Sample:** all 1–2★ reviews dated **2024-01-01 to 2026-11-20**.
  - Apple: 763 low reviews across 8 listings.
  - Google Play: 1,019 low reviews with more than 25 characters of text, across 4 apps (1,382 including one-liners).
  - Combined: **≈1,780 substantive low reviews** (2,145 including very short Play ones).
- **How themes were counted:** a keyword regex, run by script, then a manual read of every form- and signing-related review and about 150 others.
  - The theme percentages are rough. The "sign" keyword also catches "sign in", and "update" catches neutral mentions.
  - For signing and forms I give stricter counts from a second, tighter regex, plus the reviews I read by hand.
- **Recency of the Play sample:** the newest 200 per star level reach back only to **Aug 2026** for Acrobat Android and **Sep 2024** for Xodo Android, so those two reflect very recent sentiment.
- **Apple review links:** Apple has no per-review URLs. Quotes cite the app's review page plus the Apple review ID; the ID can be checked against the RSS feed.
- **Not covered:** PDF Expert has no Android app. Microsoft Store and Google Play pages would not render through the fetch tool. PDF-XChange is Windows-only and is covered from Trustpilot and forums.

**Store links used as sources:**
- Acrobat iOS: https://apps.apple.com/us/app/id469337564?see-all=reviews
- PDF Expert iOS: https://apps.apple.com/us/app/id743974925?see-all=reviews
- PDF Expert Mac: https://apps.apple.com/us/app/id1055273043?see-all=reviews
- Foxit iOS: https://apps.apple.com/us/app/id507040546?see-all=reviews
- PDFelement iOS: https://apps.apple.com/us/app/id1516765045?see-all=reviews
- PDFelement Mac: https://apps.apple.com/us/app/id1470732135?see-all=reviews
- Xodo iOS: https://apps.apple.com/us/app/id805075929?see-all=reviews
- Acrobat Android: https://play.google.com/store/apps/details?id=com.adobe.reader
- Foxit Android: https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite
- PDFelement Android: https://play.google.com/store/apps/details?id=com.wondershare.pdfelement
- Xodo Android: https://play.google.com/store/apps/details?id=com.xodo.pdf.reader

---

## Q1. What are the top recurring reasons for 1–2 star reviews?

### Takeaway
Money is behind more low ratings than any other cause. About 36–43% of low reviews mention the paywall, a subscription or the price.
- The most bitter form is **"I paid once and now it's a subscription"**, which hits PDF Expert most.
- The other common forms:
  - Do the work for free, then pay or get a watermark to save it (PDFelement, Xodo, Acrobat).
  - "Free trial" charges and hard-to-cancel plans (Acrobat, PDF Expert, Xodo).
- Next come:
  - UI and feature changes after updates (about 16–19%).
  - Crashes and bugs (about 9–14%).
  - Billing, cancellation and refund trouble (about 7–14%).
  - Lost work (about 8–12%).
  - Forced sign-in or cloud accounts (about 8%).
- AI buttons that can't be turned off are a small but growing complaint (about 4%), concentrated in Acrobat and PDF Expert in 2025–26.

### Context: ratings and review counts (fetched Nov 2026)

| App / store | Rating | Count | Low (1–2★) share of recent sample |
|---|---|---|---|
| Acrobat Reader iOS | 4.64 | 672,100 ratings | 234 of 500 newest (Jul–Nov 2026) |
| PDF Expert iOS | 4.67 | 216,746 ratings | 292 of 500 newest (Jun 2024–Nov 2026) |
| Foxit PDF Editor iOS | 4.46 | 4,763 ratings | 42 of 168 (2024+) |
| PDFelement iOS | 4.62 | 7,583 ratings | 56 of 272 (2024+) |
| Xodo iOS | 4.35 | 1,712 ratings | 48 of 86 (2024+) |
| PDF Expert Mac | n/a | Mac App Store rating not exposed by the API | 57 of 188 (2024+) |
| PDFelement Mac | n/a | Mac App Store rating not exposed by the API | 31 of 39 (2024+) |
| Acrobat Android | 4.51 | 8.09M ratings, 500M+ installs | n/a |
| Foxit Android | 3.90 | 211,804 ratings, 10M+ installs | n/a |
| PDFelement Android | 4.17 | 36,808 ratings, 5M+ installs | n/a |
| Xodo Android | 4.16 | 476,791 ratings, 10M+ installs | n/a |

- Sources: iTunes Search API (`https://itunes.apple.com/search?term=...`) and google-play-scraper, via the store links above.
- The newest-500 window is biased toward recent complaints after updates: angry users review, content users mostly don't. Overall store averages stay at 4.4–4.7.
- PDF-XChange Editor: G2 shows 4.6 across 148 reviews — [G2 via search](https://g2.com/products/pdf-xchange-editor/reviews).
- JustUseApp claims 56.9% negative sentiment from an NLP analysis of 671k Acrobat reviews. That is a third-party method and treated as low reliability — [JustUseApp](https://justuseapp.com/en/app/469337564/adobe-acrobat-reader-pdf-maker/reviews).

### Theme frequency (keyword-coded, share of low reviews)

| Theme | Apple (n=763) | Play (n=1,019) | Hit hardest |
|---|---|---|---|
| Paywall / subscription / price | 43% | 36% | PDF Expert iOS 52%, PDFelement iOS 50%, PDFelement Android 46%, Xodo Android 45% |
| UI change / removed feature after update | 19% | 16% | Xodo (25–29%), PDF Expert (25–26%) |
| Crash / bug / freeze | 14% | 9% | Foxit iOS 38%, Xodo iOS 31%, PDF Expert Mac 21% |
| Billing / cancel / refund / "scam" | 14% | 7% | PDFelement iOS 26%, Acrobat iOS 17%, PDF Expert iOS 15% |
| Lost work / files / edits | 12% | 8% | Foxit iOS 28%, Xodo iOS 27% |
| Forced sign-in / account / cloud | 8% | 8% | Foxit (iOS 23%, Android 13%), Acrobat 9% |
| Paid once, then paywalled ("used to", "already paid") | 101 of 2,145 overall (strict regex) | (included in overall) | PDF Expert iOS (42 of 292), Xodo Android (15), Acrobat iOS (10) |
| Watermark on free output | 85 of 2,145 overall | (included in overall) | Xodo Android 63, PDFelement 22 (Mac, iOS and Android) |
| Edit first, then pay to save | 25 of 2,145 overall | (included in overall) | Xodo, PDFelement Mac, Acrobat |
| Ads / upsell nags / pop-ups | 4% | 6% | Acrobat Android 10% |
| AI features | 4% | 4% | Acrobat Android 9%, Acrobat iOS 5%, PDF Expert iOS 4% |
| Support unreachable | 6% | 3% | PDFelement (iOS 21%, Mac 16%) |
| Slow / laggy | 3% | 2% | Foxit Android 4% |
| Privacy | under 1% | under 1% | n/a (privacy is usually voiced through sign-in, cloud or AI complaints instead) |

### Cited findings by theme

**1. Paid once, then paywalled ("bait and switch") — mostly PDF Expert, also Foxit, PDFelement and Xodo**
- "I paid for this app and they took all of the functionality I depend on every day and stuck it behind a subscription paywall… I cant downgrade to the last version" — PDF Expert iOS, 1★, 2026-09-30, ID 14611658489 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "I paid $40 for this app. Somehow my app got updated, and now basic functionality like filling out forms is stuck behind a subscription paywall, so that they can sell you a bunch of AI garbage." — PDF Expert Mac, 1★, 2026-09-08, ID 14526316783 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "I made a one time purchase 10 years ago… the update revoked all editing capabilities without warning." — PDF Expert iOS, 1★, 2025-11-20, ID 13422496206 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- A reviewer bought PDF Converter in 2018. Readdle revoked conversion access "first in 2025, now again in June 2026, both times forcing me to argue with support and provide old receipts" — PDF Expert iOS, 1★, 2026-07-07, ID 14273788224 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "I purchased pdf expert … used to have the ability to fill and sign … Now those features have been removed and you have to pay for a subscription" — PDF Expert iOS, 1★, 2026-06-07, ID 14154826251 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "I had purchase a perpetual license … now they have removed features that were included when I purchased the software for close to $250.00 per computer" — Foxit iOS, 1★, 2026-08-03, ID 14381733164 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
- "Bought a lifetime subscription… Got a new iPhone tried to restore it and it says I haven't paid for anything." — PDFelement iOS, 1★, 2025-10-02, ID 13216816105 — [App Store](https://apps.apple.com/us/app/id1516765045?see-all=reviews)
- "Xodo was sold. The new owners made the free version useless, with ubiquitous watermarks, ONE action per day, including printing and opening a shared doc." — Xodo Android, 1★, 2026-11-01 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
- Older (pre-2024): MacRumors users say PDF Expert's v2 "lifetime" promise ended with subscriptions and forced accounts — [MacRumors](https://forums.macrumors.com/threads/pdf-expert-switching-to-subscription-model-forces-account-creation-even-if-paid-v2-customer.2350184/page-2)
- PDF-XChange (desktop): Trustpilot reviewers say support told them that "as of May 2025 they removed access to older installers unless you have an active maintenance agreement". This rests on a single user report — [Trustpilot](https://www.trustpilot.com/review/pdf-xchange.eu), [Trustpilot CA](https://ca.trustpilot.com/review/pdf-xchange.com)

**2. Edit, then pay or get a watermark to save — PDFelement, Xodo, Acrobat**
- "Spent a bunch of time working on a document and when I went to save it found that I can't without a watermark. Definitely should be visible on the document I'm editing." — PDFelement Mac, 2★, 2024-08-28, ID 11663745594 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)
- "BECAUSE I DID NOT PURCHASE THE APP AFTER 15 MINUTES, IT PUT WATERMARKS IN BLUE ACROSS EVERY PAGE OF THE DOCUMENT (100 PAGES)" — PDFelement Mac, 1★, 2024-08-19, ID 11628878703 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)
- "Overwrites files without asking!!! … Now my original unfilled form is fully filled out WITH the watermark." — PDFelement Android, 1★, 2026-11-15 — [Google Play](https://play.google.com/store/apps/details?id=com.wondershare.pdfelement)
- "It allowed to change a doc … but when I went to save it, it required more $$$$ or it wouldn't save my project." — Acrobat iOS, 1★, 2026-11-10, ID 14773935782 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "Wanna go through all the trouble of using the app and signing a PDF only to find out that you can't save the PDF unless you PAY?????" — Acrobat iOS, 1★, 2026-08-16, ID 14435441385 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "Shouldnt be advertised as Free … App store should remove it from free search results" — PDFelement Mac, 1★, 2025-10-01, ID 13210009394 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)

**3. Free-trial charges and hard cancellation — Acrobat, PDF Expert, Xodo, PDFelement**
- "If you miss to cancel one day before it automatically renews a whole year. NO REFUNDS" — Acrobat iOS, 1★, 2026-09-24, ID 14589398513 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "they sign you up to more services that you agreed to trial on and if you didn't cancel on all of them then it's a steep cancellation fee." — Acrobat Android, 1★, 2026-11-10 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "This app double charges you, signs you out then pretends you don't have a working account." — Acrobat iOS, 1★, 2026-11-19, ID 14809499513 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "Somehow the started charging me weekly ($4.99). It's not in the Apple subscription so you can't cancel the normal way." — PDF Expert iOS, 1★, 2026-05-18, ID 14079864372 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "I clicked 'start free trial' … Turns me out I was charged immediately rather at the end of the 'free trial'." — Xodo iOS, 1★, 2024-10-18, ID 11849217300 — [App Store](https://apps.apple.com/us/app/id805075929?see-all=reviews)
- "CHARGED QUARTERLY NOT MONTHLY … I saw an option for $10/month which I thought was the correct option, and turns out it's for 'AI Credits'" — PDFelement Mac, 1★, 2024-01-15, ID 10825944136 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)
- Older (2023): "I shouldn't be charged $200 in fees just to cancel." — Acrobat iOS — [App Store](https://apps.apple.com/us/app/adobe-acrobat-reader-edit-pdf/id469337564?see-all=reviews)

**4. Forced sign-in, cloud lock-in and no offline use — Foxit, Acrobat, PDF Expert Mac**
- "Tried to open the app on a plane without internet and it doesn't even open. Also it always asks for a sign in upon loading" — Acrobat iOS, 2★, 2026-09-16, ID 14558139369 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "The process of signing-in is so important to the company that they are ready to close the application without letting a guy do his job on a PDF file before a deadline." — Acrobat Android, 1★, 2026-11-11 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "The simple act of saving a file to a phone's local hard drive shouldn't take a 20 minute long workaround session" — Acrobat iOS, 1★, 2026-11-18, ID 14806944250 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "Now I have to have an account to use the app I bought? … you can't close it until you sign into/up for an account or use the activity monitor to force close it." — PDF Expert Mac, 1★, 2025-03-30, ID 12485303208 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- A reviewer cites App Store Guideline 5.1.1(v) against the mandatory login — PDF Expert Mac, 1★, 2026-04-19, ID 13976129421 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "Why on earth does it need to be online? Why do I have to prove who I am?" — Foxit Android, 1★, 2025-12-24 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "Everytime I open a pdf file, I get a prompt to sign in with foxit account and allow notifications. I am switching to another pdf reader app." — Foxit Android, 1★, 2026-05-14 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- Adobe's forum reply on desktop Reader: the free version "leverages online capabilities that require user authentication". Sign-in prompts were reported from July 2025 — [Adobe Community](https://community.adobe.com/t5/acrobat-reader-discussions/acrobat-reader-asking-to-sign-in/m-p/15548866/highlight/true)

**5. UI redesigns, bloat and removed features after updates — Xodo, PDF Expert, PDFelement v5, Acrobat**
- "Interface is a junkyard, I don't want an account, AI, or have to dig through menus to find a clean read mode." — PDF Expert iOS, 1★, 2026-09-24, ID 14589911549 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "they've cluttered the UI with a ridiculous amount of features that are impossible to hide" — PDF Expert iOS, 2★, 2026-05-11, ID 14052708863 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "v5 is not an improvement … Why is there no option to modify the toolbars and menus?" — PDFelement iOS, 2★, 2024-11-20, ID 11972458932 — [App Store](https://apps.apple.com/us/app/id1516765045?see-all=reviews)
- "Acrobat reader is -in my view- primarily meant to quickly view a PDF document. For this purpose, I don't need AI integration, sandboxing, editing, signing…" — Acrobat Android, 2★, 2026-11-10 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "perfect example of s***ification … why is it more difficult to create a simple word document, convert to PDF, add a signature, save, and send in 2026 than it was in 2016?" — Xodo Android, 2★, 2026-11-01 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)

**6. Crashes, bugs and lost work — Foxit iOS, Xodo, PDF Expert Mac**
- "Worked with a pdf file for two hours … out of nowhere the app erases all my inputs." — Xodo iOS, 1★, 2024-08-25, ID 11650302083 — [App Store](https://apps.apple.com/us/app/id805075929?see-all=reviews)
- "literally made me fail my midterm … constantly deletes the work and edits i make" — Xodo iOS, 1★, 2024-02-24, ID 10976394529 — [App Store](https://apps.apple.com/us/app/id805075929?see-all=reviews)
- "I added some text to a document, switched to another app, switched back to Foxit, and found that half of my edits were just gone." — Foxit iOS, 1★, 2024-10-24, ID 11871617246 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
- "Used to work great. Now, it's extremely slow to open, close, and render PDF documents, and it uses absurdly large amounts of memory." — PDF Expert Mac, 2★, 2026-01-05, ID 13599371192 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "Just lost an hour's worth of highlights and notes. I literally would've been ten times better off printing the document out." — Acrobat iOS, 2★, 2026-11-04, ID 14752295410 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- File bloat that is then paywalled:
  - "a fillable pdf goes from 710kb to 21MB now … in order to reduce the file size, you have to subscribe" — PDF Expert iOS, 1★, 2026-05-03, ID 14024759987 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
  - "It blows a 750kb editable PDF up to 29mb and then reserves the size adjustment options for the paid version." — Foxit Android, 2★, 2026-03-04 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)

**7. AI features that can't be turned off — Acrobat, PDF Expert**
- "The AI button is an eyesore that can't be removed and constantly makes itself known with colorful text saying 'NEW'" — PDF Expert iOS, 1★, 2025-06-20, ID 12796322282 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "The AI button makes this look like cheap spyware. Let us remove the AI!" — PDF Expert iOS, 1★, 2026-11-15, ID 14796262014 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "No matter how many times you check the box to not open copilot automatically it still opens." — PDF Expert iOS, 2★, 2026-07-06, ID 14270482584 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "Everything is AI infested and the good useful tools are all paid for add-ons." — Acrobat iOS, 1★, 2026-11-11, ID 14778145758 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "I only open pdfs, don't create, don't need AI nonsense. Just stop." — Acrobat Android, 1★, 2026-11-19 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "I don't need AI I need rotate" — Foxit iOS, 1★, 2024-04-11, ID 11146424952 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
- Adobe Community: users report that the desktop Reader AI Assistant icon persists after AI is disabled. Adobe says it can only be hidden, not removed — [Adobe Community](https://community.adobe.com/t5/acrobat-discussions/how-to-really-get-rid-of-the-quot-ai-assistant-quot-in-adobe-reader-for-windows/m-p/15363739), [thread](https://community.adobe.com/t5/acrobat-discussions/how-can-turn-off-the-ai-assistant/m-p/14741837)

**8. Privacy and security, usually voiced through cloud or account complaints**
- "When I install this app … it automatically toggles on for syncing with iCloud. This is unacceptable and a major security risk" — Acrobat iOS, 2★, 2026-11-17, ID 14801311762 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "everything is backed up to a cloud, so if you sign medical documents on here, your social security number is now publicly visible." — Acrobat Android, 1★, 2026-11-04 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "Isn't the purpose to redact is to prevent unauthorized viewing of sensitive info?" (objecting to cloud storage) — Foxit iOS, 1★, 2025-09-09, ID 13118232174 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
- "forced to sign in before I use offline software / feels like a privacy red flag." — PDF Expert iOS, 1★, 2026-01-08, ID 13610187684 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)

**9. Support that can't be reached — PDFelement (Wondershare)**
- "they still could not figure out what I was even talking about and then they just stopped responding." — PDFelement Mac, 1★, 2024-09-23, ID 11757048986 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)
- "you are stuck with a useless AI support tool. Impossible to get help from it and impossible to get to a human." — PDFelement iOS, 1★, 2024-11-18, ID 11965394945 — [App Store](https://apps.apple.com/us/app/id1516765045?see-all=reviews)
- The website's "30 day satisfaction guarantee" turned out not to apply to perpetual licences, and the reviewer went through PayPal for a refund — PDFelement Mac, 1★, 2024-03-04, ID 11009005272 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)

### Inferences
- Users rarely object to paying as such. They object to:
  - Losing something they already had or paid for.
  - Learning the price only after doing the work.
  - Recurring billing for a utility they use occasionally.

  Every app that moved from one-time or free to subscription in 2024–26 (PDF Expert, Xodo after its sale, PDFelement) shows a wave of "used to love it" 1★ reviews.
- Forced sign-in is felt as a blocker to the core task ("before a deadline", "on a plane"), not only as a privacy issue.
- Mobile apps from desktop-strong brands (Foxit, PDF Expert iOS) are judged harshly when they fall short of their desktop versions.

### Gaps
- No Microsoft Store reviews could be read; the pages are JS-rendered and returned no review content.
- No starred store data for PDF-XChange; its evidence comes from Trustpilot and forums only.
- Reddit threads did not surface through search, which returned Adobe Community and MacRumors instead. Reddit-specific sentiment is unverified here.
- Keyword coding is approximate; a human-coded subsample would tighten the percentages.

---

## Q2. Which complaints relate specifically to form filling and signing?

### Takeaway
- **Scale:** strict matching found **about 47 form complaints and about 59 real signature complaints** (excluding "sign in") among the 2,145 low reviews, roughly 2–3% each. They are a minority, but every one comes from a user who failed at the exact task they installed the app for.
- **Forms**, in order of how often they come up:
  1. Form filling paywalled or watermarked.
  2. Can't type in fields, or fields stop working after an update.
  3. Flat (non-fillable) PDFs can't be filled, or the text box is hard to place, resize or re-edit.
  4. Filled data lost on save, export or flatten.
  5. XFA and JavaScript forms unsupported.
  6. File-size bloat.
  7. Linked fields.
- **Signing**, in order of how often it comes up:
  1. Drawn signatures paywalled or missing.
  2. The signature disappears after export or send.
  3. Signing crashes or is broken.
  4. No certificate, CAC or PKI support where users expected it.
  5. Certificate validity display (yellow "?" instead of a green tick).

### Cited findings — FORM FILLING

**Fields not working or can't type (often after an update):**
- "when I tap a form field to start typing the app doesn't let me write." — PDF Expert iOS, 1★, 2025-11-27, ID 13447163086 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "Ever since the new update PDF Expert has issues with displaying items within fillable fields … Works just fine on Adobe. Works just fine on Preview." — PDF Expert iOS (iPadOS 26), 1★, 2026-01-04, ID 13595522242 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "when completing a form, it will crash. Especially if I'm trying to input a date … in a designated field" — PDF Expert iOS, 2★, 2026-02-08, ID 13729078619 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "Piece of junk, no way to fill in text into fields." — Xodo Android, 1★, 2026-10-29 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
- "After my 10th attempt at filling out the same form and it not work I'm giving up. Now I have to go find a printer." — Acrobat iOS, 1★, 2026-08-17, ID 14438548654 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "Would not let me edit a form at all" — Acrobat iOS, 1★, 2026-08-01, ID 14374943701 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- Desktop Acrobat/Reader:
  - Clicking a field opens a "request e-signature" menu instead of a cursor. Tab still works, and Adobe staff called it a bug in the latest version — [Adobe Community](https://community.adobe.com/t5/acrobat-discussions/adobe-pdf-prompting-requiring-signatures-for-any-and-all-text-fields-not-document-specific/td-p/14574753), [thread](https://community.adobe.com/t5/acrobat-discussions/fillable-form-won-t-let-me-type-unless-i-right-click-then-tab-through/m-p/14564559)

**Flat or scanned (non-fillable) PDFs and placing text boxes:**
- "Turns out if it wasn't set up as a form to begin with, this app is useless." — PDFelement Android, 1★, 2025-06-10 — [Google Play](https://play.google.com/store/apps/details?id=com.wondershare.pdfelement)
- "It took me ages to get to the correct position when editing text and once you have typed something, you cannot move the text box nor you can edit it again." — Foxit Android, 1★, 2026-08-04 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "I just wanted to fill out ONE simple non-fillable PDF file which seems too much to ask in this day and age" — Foxit Android, 1★, 2026-08-29 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "the app does not allow you to move text boxes where you want them. You drag to a location and it will stick to an entirely different column" — PDFelement iOS, 1★, 2024-01-04, ID 10784376911 — [App Store](https://apps.apple.com/us/app/id1516765045?see-all=reviews)
- "No way to drag and drop text so you need to hope it will detect the correct size of the text box, it never does" — PDF Expert iOS, 1★, 2026-07-13, ID 14297937166 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "tapping on the boxes to be filled will just select those boxes" — PDFelement Android, 1★, 2026-02-12 — [Google Play](https://play.google.com/store/apps/details?id=com.wondershare.pdfelement)
- **Font and size of typed text** (no complaints about misalignment or wrong baselines as such):
  - "Settings for text size and font do not get saved, the app defaults to Courier 18pt" — Foxit iOS, 1★, 2026-03-23, ID 13879032212 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
  - "Almost impossible to change FONT SIZE in TEXT EDIT … This took all morning to do a form I should have been able to complete in 15 minutes." — PDF Expert Mac, 2★, 2024-06-04, ID 11343790392 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "I spent suns time filling out a form just to see I made a mistake … I could not go back to it and correct anything." The checkmark tool "doesn't make a check at all" — Foxit iOS, 1★, 2024-01-27, ID 10872658122 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)

**Filled data lost on save, export or flatten:**
- "I open a form, fill it with info, export it with flatten ... Aaand all the info I painstakingly put in is gone … this has like 10 AI buttons but no save button ???" — PDFelement Android, 1★, 2025-11-23 — [Google Play](https://play.google.com/store/apps/details?id=com.wondershare.pdfelement)
- "constantly having to redo paperwork because Xodo wouldn't save anything or save a 'flattened' copy that is completely blank." — Xodo iOS, 1★, 2026-04-10, ID 13944081201 — [App Store](https://apps.apple.com/us/app/id805075929?see-all=reviews)
- "deleting all of my progress on forms randomly and autosaving over my backups" — Foxit Android, 1★, 2025-12-01 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "in the middle of filling something, they say 'the file could not be saved' and whatever I had pressed saved before has also gone" — Acrobat Android, 1★, 2026-11-19 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "Crashes often losing pdf form data" — Foxit Android, 2★, 2024-11-07 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "Can't save forms with added text." — Foxit Android, 2★, 2026-01-22 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- Comment text boxes don't print or export: "There is no way to just check a button to convert the comment text boxes to permanently show in printed format." — Acrobat Android, 1★, 2026-11-13 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- Older (undated, pre-2024 likely): Acrobat's "done" button in Fill & Sign is easy to hit by mistake, losing work — [App Store](https://apps.apple.com/us/app/adobe-acrobat-reader-edit-pdf/id469337564?see-all=reviews)

**Form filling paywalled or watermarked:**
- "basic functionality like filling out forms is stuck behind a subscription paywall" — PDF Expert Mac, 1★, 2026-09-08, ID 14526316783 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "I can no longer fill in forms or add signatures. The only thing I can do now is view and save documents." — PDF Expert iOS, 1★, 2024-06-12, ID 11373747461 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "Why on gods green earth is putting BLANK TEXT BOXES onto my PDF locked behind a PAYWALL." — PDF Expert iOS, 1★, 2026-07-27, ID 14356881690 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "You have to have a recurring membership to fill out a form and save." — PDFelement Android, 1★, 2025-09-22 — [Google Play](https://play.google.com/store/apps/details?id=com.wondershare.pdfelement)
- "Let me fill a form and not show to the world that I used Xodo." (watermark) — Xodo Android, 1★, 2026-07-31 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
- "the watermark … can make docs like filling in a form look suspect to those receiving it" — Xodo Android, 2★, 2025-04-28 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
- "buy this other temporary thing that cost $40 so you can fill out one document." — Acrobat iOS, 1★, 2026-09-10, ID 14534187851 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- Creating fillable forms paywalled without warning — PDFelement Mac, 1★, 2025-09-30, ID 13205310475 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)

**XFA and JavaScript forms:**
- "Xodo does not work with forms that use JavaScript. All but the most basic forms use JavaScript." — Xodo iOS, 1★, 2025-04-12, ID 12534514771 — [App Store](https://apps.apple.com/us/app/id805075929?see-all=reviews)
- "you need subscription even to read PDFs with xfa format" — Foxit Android, 1★, 2026-09-23 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "XFA crashes app. downloaded this app specifically for its ability to read and edit XFA Files" — Foxit Android, 1★, 2025-12-02 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)

**Linked fields and file bloat:**
- "the fields are linked and I cant change that on mobile. When I enter info for the first form, it autofills the fields in the second form." — Foxit Android, 2★, 2025-05-07 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- File-size bloat on fillable PDFs: see the 710 KB → 21 MB (PDF Expert) and 750 KB → 29 MB (Foxit) quotes under Q1, theme 6.
- **Comb fields:** none of the sampled reviews mention them.

### Cited findings — SIGNING

**Drawn signatures paywalled or missing:**
- "in order to use my iPad pen to write my signature. That feature is locked out unless I have the premium subscription." — Acrobat iOS, 2★, 2026-10-12, ID 14657750581 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "$40 to sign a pdf" — PDF Expert iOS, 1★, 2025-09-04, ID 13100219129 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "It will tell you to download a companion app just to [put] your signature. Both apps require a subscription." — Foxit Android, 1★, 2026-06-11 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "The free version doesn't include adding your signature, so it is only for testing." — Foxit Android, 2★, 2026-01-06 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "I buy the app for the document signing feature only to find that is an added cost that they won't identify. I must call a phone number for a sales pitch." — Foxit iOS, 2★, 2024-01-05, ID 10786933972 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
- "Every time I open up FoxIt, I get nagged about my eSign trial expiring. I purchased the mobile yearly subscription so I could fill out a PDF form." — Foxit Android, 2★, 2026-05-14 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
- "Cannot longer fill and sign PDF's without subscription" — Xodo Android, 1★, 2026-08-27 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
- "I can't stamp or sign all pages at once even after paying the upgraded version." — Foxit Android, 1★, 2026-11-19 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)

**Signature disappears or is altered on export:**
- "I can only see my signature when viewing the PDF using this app. When I export to iCloud or email, the signature gets removed." — PDF Expert iOS, 2★, 2025-04-22, ID 12574259919 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "After signing a document from my app and sending it to other people, they would open the document and see a document that's not signed." — Acrobat Android, 2★, 2026-10-20 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "disfigures signatures when editing a document" — Acrobat iOS, 1★, 2026-08-27, ID 14479888680 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "twice this week it would not save edits, annotations, or signatures correctly. Wound up using Preview" — PDF Expert iOS, 1★, 2025-06-28, ID 12827722226 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "Trying to sign documents on my iPad and the app kept wiping the changes I made" — PDF Expert iOS, 1★, 2024-10-13, ID 11830801436 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- "No longer usable for fill and sign. It just keeps using red as a default color." — Xodo Android, 2★, 2025-04-08 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)

**Signing crashes or is broken:**
- "Keeps shutting down when signing!" — Acrobat Android, 1★, 2026-11-06 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "I am not longer able to fill & sign! This app is broken." — Acrobat Android, 1★, 2026-10-31 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "it keeps giving me this error (clear cache) with pre saved signature" — Acrobat Android, 2★, 2026-10-09 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "I can't use the signature feature. I have to use the initials." — Acrobat iOS, 1★, 2026-11-09, ID 14769920338 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "my SIMPLE pdf files can not be signed in IOS devices, like IPad. Automatically converts them to READ only." (paid subscriber) — Acrobat iOS, 1★, 2026-10-06, ID 14633730044 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- "App crashes instantly…I can't even open a pdf or do a signature" — Foxit iOS, 1★, 2025-09-18, ID 13152325985 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
- "I paid for the lifetime version of Foxit but it doesn't work anymore. I can't even sign a PDF." — Foxit iOS, 1★, 2024-02-21, ID 10965572418 — [App Store](https://apps.apple.com/us/app/id507040546?see-all=reviews)
- Adobe's standalone Fill & Sign app was announced for retirement in March 2025, with users told to move to Acrobat Reader — [Adobe Community](https://community.adobe.com/announcements-14/adobe-retires-fill-sign-switch-to-acrobat-reader-6729). A related review: "My Adobe fill & sign stopped working so I'm looking for alternatives." — PDF Expert iOS, 1★, 2025-03-08, ID 12397621815 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)

**Digital certificate, CAC and PKI signing:**
- "what they mean is you can put a picture that acts as a proxy for your signature. It does not mean that you can use a certificate to provide a digital signature" — PDF Expert Mac, 1★, 2026-08-15, ID 14432771459 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "DO NOT BUY IF YOU NEED CAC SIGNATURE ABILITY … digitally sign documents via digital certificates/common access cards this is NOT SUPPORTED … refund (~45 days post purchase) … told to kick rocks (out ~$140)" — PDF Expert Mac, 1★, 2025-05-21, ID 12681656186 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "there's no easy or obvious way to import a digital certificate, despite what it says in the app description." — PDF Expert Mac, 1★, 2025-07-27, ID 12944882573 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- "Trying to lock the form after signing a PDF is unnecessarily hard, and doing it with a certificate isn't easy either. My certificate shows that it is outdated" — Acrobat Android, 2★, 2026-11-18 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- "digital signature verification option enable not showing" — Acrobat Android, 1★, 2026-11-03 — [Google Play](https://play.google.com/store/apps/details?id=com.adobe.reader)
- **Validation display (India, Aadhaar and government certificates):**
  - "Digital Signature is not getting verified" — Foxit Android, 2★, 2026-10-04 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
  - "showing that that signature is verified but not replacing it with green tick" — Foxit Android, 2★, 2026-05-16 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
  - "sign verified, but green tick missing" — Foxit Android, 1★, 2025-11-05 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)

**Signatures invalidated or locking the form:**
- Desktop Reader users can't edit fields after "Sign Yourself" and saving. The answer given: "You can't edit a signed file. You will need to invalidate your signature first." — [Adobe Community](https://community.adobe.com/t5/acrobat-reader-discussions/can-t-edit-or-fill-pdf-form-fields-after-signing-and-saving-reader-dc-free/td-p/13023568)
- None of the sampled store reviews used the word "invalidated".

### Inferences
- **Forms:** the typical failure is not field detection itself, which almost no one mentions. It is one of three things:
  - The task gets paywalled or watermarked midway.
  - Typed text can't be positioned, sized or re-edited on flat PDFs.
  - Data is lost on save, export or flatten.

  Reliable save, clean (unbranded) output, and easy free-text placement with sensible fonts would address most of these complaints.
- **Signing:** two separate audiences complain.
  - Consumers want a free, drawn or typed signature that survives export.
  - Professionals (US military and government CAC users, Indian Aadhaar and DSC users) want real certificate signing and correct validation display. They feel misled when "sign" means only an image (PDF Expert Mac).

### Gaps
- Comb fields and misalignment of field text (baseline or font scaling inside widgets) were not found in this sample. That could mean they are rare in store reviews, or that users describe them vaguely ("form doesn't work").
- Few complaints about signatures being invalidated in store reviews; this mostly appears on desktop forums.

---

## Q3. What do reviewers say they wanted instead?

### Takeaway
Reviewers repeatedly ask for six things:
1. **A one-time purchase**, and for past purchases to be honoured.
2. **Basic fill, sign and save free, without a watermark.**
3. **No forced account; work offline.**
4. **Price shown before the work is done.**
5. **A way to hide AI and new-feature clutter.**
6. **Reliability: never lose edits.**

Several name the free alternatives they moved to: Apple Preview, Skim, PDFgear, LibreOffice/"Libre", GoodNotes, and simply printing the form.

### Cited findings
- **One-time pricing:**
  - "I would happily pay a reasonable one time fee to have a good app" — PDF Expert iOS, 2★, 2025-03-26, ID 12467142157 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
  - "I do not mind paying a one time fee for an app I will use for a few years but really hate the subscription model" — PDF Expert iOS, 1★, 2025-03-07, ID 12393514569 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
  - "I'd be open to a one-time fee, but the monthly subscription fee is r[idiculous]" — Xodo Android, 2★, 2025-04-28 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
  - Keyword counts across the 2,145 low reviews: "one-time / one time purchase / payment / fee" about 29 mentions; "watermark" 85; "just want" or "all I want" 22.
- **Honour past purchases:**
  - "I just want the features I already paid for." — PDF Expert iOS, 1★, 2024-06-12, ID 11373747461 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- **Honest pricing up front:**
  - "If you had to;d me upfront what the cost is I might buy it." — PDFelement iOS, 1★, 2025-07-30, ID 12954801658 — [App Store](https://apps.apple.com/us/app/id1516765045?see-all=reviews)
  - "I'm not interested in doing business with a company that can't be upfront about all the cost" — Acrobat iOS, 1★, 2026-11-10, ID 14773935782 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- **Trial without a subscription trap:**
  - "There should be a 3 day free trial without signing up for a subscription you have to cancel." — Xodo Android, 2★, 2026-04-28 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
  - "How about you give me a free trial for 7 days. If I like it then I will subscribe." — PDF Expert iOS, 1★, 2024-07-14, ID 11493605054 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- **No account, viewer stays a viewer:**
  - "Asks you to sign in. I just want to view pdfs" — Foxit Android, 1★, 2025-11-26 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
  - "Looking for alternatives that don't block me from accessing my own documents" — Acrobat iOS, 2★, 2026-09-16, ID 14558139369 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
- **Let users turn things off:**
  - "please for the love of god let me turn it off." (AI button) — PDF Expert iOS, 1★, 2025-06-20, ID 12796322282 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
  - "a button allowing for us LITERALLY POOR folk to say no thanks and store that in its memory banks permanently" (upsell nag) — Xodo Android, 1★, 2026-03-02 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
  - "Linked fields should have a toggle." — Foxit Android, 2★, 2025-05-07 — [Google Play](https://play.google.com/store/apps/details?id=com.foxit.mobile.pdf.lite)
  - "Why is there no option to modify the toolbars and menus?" — PDFelement iOS, 2★, 2024-11-20, ID 11972458932 — [App Store](https://apps.apple.com/us/app/id1516765045?see-all=reviews)
- **Show the watermark before the user starts:**
  - "Definitely should be visible on the document I'm editing." — PDFelement Mac, 2★, 2024-08-28, ID 11663745594 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)
- **Some would accept ads instead of a subscription:**
  - "I would be perfectly fine watching a 30 second ad every once in a while but expecting me to pay a subscription…" — PDFelement iOS, 1★, 2025-05-21, ID 12682837231 — [App Store](https://apps.apple.com/us/app/id1516765045?see-all=reviews)
- **Alternatives named:**
  - Skim: "there's an open source, free MacOS app called 'Skim' that does everything the paid version of PDF Expert used to do." — PDF Expert Mac, 1★, 2024-08-10, ID 11595455194 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
  - PDFgear: a German reviewer switched from Xodo — Xodo Android, 1★, 2026-04-01 — [Google Play](https://play.google.com/store/apps/details?id=com.xodo.pdf.reader)
  - "Libre would do this WITHOUT watermarking" — PDFelement Mac, 2★, 2025-08-24, ID 13057237614 — [Mac App Store](https://apps.apple.com/us/app/id1470732135?see-all=reviews)
  - Apple Preview, as the fallback that worked: PDF Expert iOS IDs 12827722226 and 13595522242 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
- **Price or availability on other platforms:**
  - "Where's the Mac version of the app that will sync with iCloud??" — Xodo iOS, 2★, 2025-03-22, ID 12451704388 — [App Store](https://apps.apple.com/us/app/id805075929?see-all=reviews)
  - One purchase should cover all devices: "Bought PDF Expert app as a one time purchase but now it only works on on my MacBook" — PDF Expert Mac, 1★, 2025-08-16, ID 13025546181 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)
- **Several reviews say outright what would have earned 4–5★:**
  - "If I weren't paying for this, I'd give 4" (about bugs) — PDF Expert iOS, 1★, 2025-03-26, ID 12467205933 — [App Store](https://apps.apple.com/us/app/id743974925?see-all=reviews)
  - "Would been 5 but, I Hate who you've become." — Acrobat iOS, 1★, 2026-11-17, ID 14802443019 — [App Store](https://apps.apple.com/us/app/id469337564?see-all=reviews)
  - "Otherwise 5-star app kept nagging me for a review, so it got one." — PDF Expert Mac, 1★, 2026-02-10, ID 13734681230 — [Mac App Store](https://apps.apple.com/us/app/id1055273043?see-all=reviews)

### Inferences
- These are positioning opportunities for a challenger app, as the reviews themselves state them:
  - Offline, account-free, one-time or perpetual licence.
  - Free fill, sign and save with no watermark.
  - Visible pricing.
  - Real certificate signing, for the professional segment.
- Many 1★ reviews come from formerly loyal users ("used to love"). The ratings dropped because of trust broken by business-model changes, not because the core software fell short.

### Gaps
- No quantitative "would raise my rating if…" data. The wishes above are inferred from what reviewers explicitly ask for, counted by keyword only.
- Microsoft Store (Windows) users' wishes are not covered.
