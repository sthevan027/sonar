import logo from "../src-tauri/icons/128x128.png";

// Esqueleto: só prova a tipografia, as cores e o recorte do painel.
export default function App() {
  return (
    <main
      style={{
        width: 388,
        height: 668,
        margin: "16px auto",
        boxSizing: "border-box",
        background: "var(--panel)",
        border: "1px solid var(--line)",
        borderRadius: 18,
        boxShadow: "0 24px 60px rgba(0, 0, 0, 0.55)",
        padding: 18,
        display: "flex",
        flexDirection: "column",
        alignItems: "center",
        justifyContent: "center",
        gap: 12,
        color: "var(--text)",
        fontFamily: "var(--font-text)",
      }}
    >
      <img src={logo} alt="" width={96} height={96} />
      <h1
        style={{
          margin: 0,
          fontFamily: "var(--font-display)",
          fontWeight: 700,
          fontSize: 32,
          letterSpacing: "-0.01em",
        }}
      >
        Sonar
      </h1>
      <p style={{ margin: 0, color: "var(--mute)", fontSize: 13 }}>
        Controle de áudio por app
      </p>
      <span style={{ fontFamily: "var(--font-mono)", color: "var(--brass)", fontSize: 12 }}>
        #D9A441
      </span>
    </main>
  );
}
