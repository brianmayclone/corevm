import { useState } from 'react';
import { getInitialLang, persistLang } from './i18n';
import type { Lang } from './i18n';
import Navbar from './components/Navbar';
import Hero from './components/Hero';
import Appliance from './components/Appliance';
import Features from './components/Features';
import Screenshots from './components/Screenshots';
import Cluster from './components/Cluster';
import CoreSAN from './components/CoreSAN';
import Architecture from './components/Architecture';
import VMManager from './components/VMManager';
import CTA from './components/CTA';
import Footer from './components/Footer';
import ComingSoonModal from './components/ComingSoonModal';
import LegalModal from './components/LegalModal';
import type { LegalPage } from './components/LegalModal';

export default function App() {
  const [lang, setLang] = useState<Lang>(getInitialLang);
  const [showDownloadError, setShowDownloadError] = useState(false);
  const [showComingSoon, setShowComingSoon] = useState(false);
  const [legalPage, setLegalPage] = useState<LegalPage>(null);

  const handleLangChange = (newLang: Lang) => {
    setLang(newLang);
    persistLang(newLang);
    document.documentElement.lang = newLang;
  };

  const onDownloadClick = async () => {
    try {
      const res = await fetch('https://api.github.com/repos/brianmayclone/corevm/releases');
      if (!res.ok) throw new Error('Failed to fetch releases');
      const releases = await res.json();
      const latest = releases[0];
      if (!latest) throw new Error('No releases found');
      const isoAsset = latest.assets?.find((a: { name: string }) => a.name.endsWith('.iso'));
      if (!isoAsset) throw new Error('No ISO asset found');
      window.location.href = isoAsset.browser_download_url;
    } catch {
      setShowDownloadError(true);
    }
  };

  return (
    <div className="min-h-screen bg-surface-950 text-white">
      <Navbar lang={lang} onLangChange={handleLangChange} onDownloadClick={onDownloadClick} />
      <main>
        <Hero lang={lang} onDownloadClick={onDownloadClick} />
        <Appliance lang={lang} />
        <Features lang={lang} />
        <Screenshots lang={lang} />
        <Cluster lang={lang} />
        <CoreSAN lang={lang} />
        <Architecture lang={lang} />
        <VMManager lang={lang} onDownloadClick={() => setShowComingSoon(true)} />
        <CTA lang={lang} onDownloadClick={onDownloadClick} />
      </main>
      <Footer lang={lang} onLegalClick={setLegalPage} />
      <ComingSoonModal open={showDownloadError} onClose={() => setShowDownloadError(false)} lang={lang} variant="download-error" />
      <ComingSoonModal open={showComingSoon} onClose={() => setShowComingSoon(false)} lang={lang} />
      <LegalModal page={legalPage} onClose={() => setLegalPage(null)} lang={lang} />
    </div>
  );
}
