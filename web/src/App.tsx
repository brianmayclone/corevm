import { useState } from 'react';
import { getInitialLang, persistLang } from './i18n';
import type { Lang } from './i18n';
import Navbar from './components/Navbar';
import Hero from './components/Hero';
import Appliance from './components/Appliance';
import Features from './components/Features';
import Screenshots from './components/Screenshots';
import Cluster from './components/Cluster';
import Architecture from './components/Architecture';
import VMManager from './components/VMManager';
import CTA from './components/CTA';
import Footer from './components/Footer';
import ComingSoonModal from './components/ComingSoonModal';
import LegalModal from './components/LegalModal';
import type { LegalPage } from './components/LegalModal';

export default function App() {
  const [lang, setLang] = useState<Lang>(getInitialLang);
  const [showComingSoon, setShowComingSoon] = useState(false);
  const [legalPage, setLegalPage] = useState<LegalPage>(null);

  const handleLangChange = (newLang: Lang) => {
    setLang(newLang);
    persistLang(newLang);
    document.documentElement.lang = newLang;
  };

  const onDownloadClick = () => setShowComingSoon(true);

  return (
    <div className="min-h-screen bg-surface-950 text-white">
      <Navbar lang={lang} onLangChange={handleLangChange} onDownloadClick={onDownloadClick} />
      <main>
        <Hero lang={lang} onDownloadClick={onDownloadClick} />
        <Appliance lang={lang} />
        <Features lang={lang} />
        <Screenshots lang={lang} />
        <Cluster lang={lang} />
        <Architecture lang={lang} />
        <VMManager lang={lang} onDownloadClick={onDownloadClick} />
        <CTA lang={lang} onDownloadClick={onDownloadClick} />
      </main>
      <Footer lang={lang} onLegalClick={setLegalPage} />
      <ComingSoonModal open={showComingSoon} onClose={() => setShowComingSoon(false)} lang={lang} />
      <LegalModal page={legalPage} onClose={() => setLegalPage(null)} lang={lang} />
    </div>
  );
}
