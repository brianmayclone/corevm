import { motion } from 'framer-motion';
import { Cpu, MemoryStick, Binary, Plug } from 'lucide-react';
import type { Lang } from '../i18n';
import { t } from '../i18n';

interface ArchitectureProps {
  lang: Lang;
}

const items = [
  { icon: Cpu, title: 'arch_cpu_title', desc: 'arch_cpu_desc', color: 'primary' },
  { icon: MemoryStick, title: 'arch_memory_title', desc: 'arch_memory_desc', color: 'accent' },
  { icon: Binary, title: 'arch_bios_title', desc: 'arch_bios_desc', color: 'primary' },
  { icon: Plug, title: 'arch_ffi_title', desc: 'arch_ffi_desc', color: 'accent' },
] as const;

export default function Architecture({ lang }: ArchitectureProps) {
  return (
    <section id="architecture" className="relative py-24 md:py-32">
      <div className="pointer-events-none absolute inset-0 bg-gradient-to-b from-transparent via-surface-900/50 to-transparent" />

      <div className="relative mx-auto max-w-7xl px-6">
        {/* Header */}
        <div className="mx-auto max-w-2xl text-center">
          <motion.span
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            className="inline-block rounded-full border border-primary-500/20 bg-primary-500/10 px-3 py-1 text-xs font-semibold uppercase tracking-wider text-primary-300"
          >
            {t(lang, 'arch_badge')}
          </motion.span>
          <motion.h2
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.1 }}
            className="mt-4 text-3xl font-extrabold tracking-tight text-white sm:text-4xl md:text-5xl"
          >
            {t(lang, 'arch_title')}
          </motion.h2>
          <motion.p
            initial={{ opacity: 0, y: 10 }}
            whileInView={{ opacity: 1, y: 0 }}
            viewport={{ once: true }}
            transition={{ delay: 0.2 }}
            className="mt-4 text-lg text-surface-400"
          >
            {t(lang, 'arch_subtitle')}
          </motion.p>
        </div>

        {/* Architecture diagram / stack */}
        <div className="mt-16 grid gap-6 md:grid-cols-2">
          {items.map((item, i) => {
            const Icon = item.icon;
            const isPrimary = item.color === 'primary';
            return (
              <motion.div
                key={item.title}
                initial={{ opacity: 0, y: 20 }}
                whileInView={{ opacity: 1, y: 0 }}
                viewport={{ once: true }}
                transition={{ delay: i * 0.1 }}
                className="group relative overflow-hidden rounded-2xl border border-white/5 bg-white/[0.02] p-8 transition-all hover:border-white/10"
              >
                {/* Accent line on top */}
                <div className={`absolute inset-x-0 top-0 h-px ${isPrimary ? 'bg-gradient-to-r from-transparent via-primary-500/50 to-transparent' : 'bg-gradient-to-r from-transparent via-accent-400/50 to-transparent'}`} />

                <div className={`mb-5 inline-flex rounded-xl border p-3 ${isPrimary ? 'border-primary-500/20 bg-primary-500/10' : 'border-accent-400/20 bg-accent-400/10'}`}>
                  <Icon size={22} className={isPrimary ? 'text-primary-400' : 'text-accent-400'} />
                </div>
                <h3 className="text-lg font-bold text-white">{t(lang, item.title)}</h3>
                <p className="mt-2 text-sm leading-relaxed text-surface-400">{t(lang, item.desc)}</p>
              </motion.div>
            );
          })}
        </div>

        {/* Device table */}
        <motion.div
          initial={{ opacity: 0, y: 20 }}
          whileInView={{ opacity: 1, y: 0 }}
          viewport={{ once: true }}
          className="mt-12 overflow-hidden rounded-2xl border border-white/5 bg-white/[0.02]"
        >
          <div className="overflow-x-auto">
            <table className="w-full text-left text-sm">
              <thead>
                <tr className="border-b border-white/5">
                  <th className="px-6 py-4 text-xs font-semibold uppercase tracking-wider text-surface-400">Category</th>
                  <th className="px-6 py-4 text-xs font-semibold uppercase tracking-wider text-surface-400">Devices</th>
                </tr>
              </thead>
              <tbody className="divide-y divide-white/5">
                {[
                  ['Storage', 'AHCI (SATA), IDE/ATA, Disk Cache'],
                  ['Network', 'Intel E1000 (82540EM), VirtIO Net, SLIRP NAT'],
                  ['GPU', 'VGA/Bochs VBE, VirtIO GPU, Intel HD Graphics 530'],
                  ['Audio', 'AC\'97 (ICH, 8086:2415)'],
                  ['Input', 'PS/2 Keyboard/Mouse, VirtIO Input (Keyboard, Tablet)'],
                  ['Interrupts', 'Dual 8259A PIC, Local APIC, I/O APIC, HPET, 8254 PIT'],
                  ['I/O', '16550 UART (COM1-4), UHCI USB 1.1, DMA, SMBus'],
                  ['System', 'CMOS/RTC, PCI Bus, Q35 MCH, ACPI, APM'],
                  ['Firmware', 'fw_cfg, Custom NASM BIOS, SeaBIOS'],
                ].map(([cat, devices]) => (
                  <tr key={cat} className="transition-colors hover:bg-white/[0.02]">
                    <td className="whitespace-nowrap px-6 py-3 font-medium text-white">{cat}</td>
                    <td className="px-6 py-3 font-mono text-xs text-surface-400">{devices}</td>
                  </tr>
                ))}
              </tbody>
            </table>
          </div>
        </motion.div>
      </div>
    </section>
  );
}
