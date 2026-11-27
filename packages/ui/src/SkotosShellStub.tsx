import styles from "./SkotosShellStub.module.css";

export interface SkotosShellStubProps {
  title: string;
  subtitle?: string;
}

export function SkotosShellStub({ title, subtitle }: SkotosShellStubProps) {
  return (
    <main className={styles.shell}>
      <section className={styles.card}>
        <h1 className={styles.title}>{title}</h1>
        {subtitle ? <p className={styles.subtitle}>{subtitle}</p> : null}
      </section>
    </main>
  );
}
