//! Complete, compile-time checked UI vocabulary. Provider strings, addresses,
//! hostnames and measured values are deliberately never translated.
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Language {
    #[default]
    En,
    De,
    Hu,
    Fr,
    It,
    Ru,
}

impl Language {
    pub const ALL: [Self; 6] = [Self::En, Self::De, Self::Hu, Self::Fr, Self::It, Self::Ru];
    pub const fn index(self) -> usize {
        match self {
            Self::En => 0,
            Self::De => 1,
            Self::Hu => 2,
            Self::Fr => 3,
            Self::It => 4,
            Self::Ru => 5,
        }
    }
    pub const fn code(self) -> &'static str {
        ["en", "de", "hu", "fr", "it", "ru"][self.index()]
    }
    pub const fn name(self) -> &'static str {
        [
            "English",
            "Deutsch",
            "Magyar",
            "Français",
            "Italiano",
            "Русский",
        ][self.index()]
    }
    pub fn from_code(code: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|v| v.code() == code)
            .unwrap_or_default()
    }
    pub const fn text(self, key: Key) -> &'static str {
        key.translations()[self.index()]
    }
    /// Replace named placeholders. UI code supplies already formatted values.
    /// This avoids positional argument order assumptions between languages.
    pub fn format(self, key: Key, values: &[(&str, &str)]) -> String {
        let mut out = self.text(key).to_owned();
        for (name, value) in values {
            out = out.replace(&format!("{{{name}}}"), value);
        }
        out
    }
}
impl fmt::Display for Language {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.name())
    }
}

macro_rules! vocabulary {
    ($($key:ident => [$en:literal, $de:literal, $hu:literal, $fr:literal, $it:literal, $ru:literal]),* $(,)?) => {
        #[derive(Debug, Clone, Copy, PartialEq, Eq)]
        pub enum Key { $($key),* }
        impl Key {
            pub const ALL: &'static [Key] = &[$(Self::$key),*];
            pub const fn translations(self) -> [&'static str; 6] {
                match self { $(Self::$key => [$en, $de, $hu, $fr, $it, $ru]),* }
            }
        }
    }
}

vocabulary! {
    Tagline => ["Observed networks. Approximate places.", "Beobachtete Netze. Ungefähre Orte.", "Megfigyelt hálózatok. Becsült helyek.", "Réseaux observés. Lieux approximatifs.", "Reti osservate. Posizioni approssimative.", "Наблюдаемые сети. Примерные местоположения."],
    Target => ["Target", "Ziel", "Célpont", "Cible", "Destinazione", "Цель"],
    TargetHint => ["Hostname, IP address or URL", "Hostname, IP-Adresse oder URL", "Gépnév, IP-cím vagy URL", "Nom d’hôte, adresse IP ou URL", "Nome host, indirizzo IP o URL", "Имя узла, IP-адрес или URL"],
    Start => ["Start", "Starten", "Indítás", "Démarrer", "Avvia", "Начать"],
    Stop => ["Stop", "Stoppen", "Leállítás", "Arrêter", "Ferma", "Остановить"],
    TraceAgain => ["Trace again", "Route erneut ermitteln", "Útvonal újramérése", "Relancer le traçage", "Ripeti tracciamento", "Повторить трассировку"],
    Replay => ["Replay", "Erneut abspielen", "Újrajátszás", "Rejouer", "Riproduci", "Повторить показ"],
    Pause => ["Pause animation", "Animation pausieren", "Animáció szüneteltetése", "Suspendre l’animation", "Pausa animazione", "Приостановить анимацию"],
    Resume => ["Resume animation", "Animation fortsetzen", "Animáció folytatása", "Reprendre l’animation", "Riprendi animazione", "Продолжить анимацию"],
    SingleTarget => ["Single target", "Einzelziel", "Egy célpont", "Cible unique", "Destinazione singola", "Одна цель"],
    MultiTarget => ["Multiple targets", "Mehrere Ziele", "Több célpont", "Cibles multiples", "Più destinazioni", "Несколько целей"],
    ConfigureTargets => ["Configure targets", "Ziele konfigurieren", "Célpontok beállítása", "Configurer les cibles", "Configura destinazioni", "Настроить цели"],
    TargetListHint => ["Configure 1–10 targets. Each target runs independently.", "1–10 Ziele konfigurieren. Jedes Ziel wird unabhängig geprüft.", "Adjon meg 1–10 célpontot. A mérések egymástól függetlenek.", "Configurez 1 à 10 cibles. Chaque cible est mesurée séparément.", "Configura da 1 a 10 destinazioni. Ognuna viene misurata separatamente.", "Укажите от 1 до 10 целей. Измерения выполняются независимо."],
    AddTarget => ["Add target", "Ziel hinzufügen", "Célpont hozzáadása", "Ajouter une cible", "Aggiungi destinazione", "Добавить цель"],
    Remove => ["Remove", "Entfernen", "Eltávolítás", "Retirer", "Rimuovi", "Удалить"],
    Apply => ["Apply", "Übernehmen", "Alkalmazás", "Appliquer", "Applica", "Применить"],
    Cancel => ["Cancel", "Abbrechen", "Mégse", "Annuler", "Annulla", "Отмена"],
    Close => ["Close", "Schließen", "Bezárás", "Fermer", "Chiudi", "Закрыть"],
    Save => ["Save", "Speichern", "Mentés", "Enregistrer", "Salva", "Сохранить"],
    Settings => ["Settings", "Einstellungen", "Beállítások", "Paramètres", "Impostazioni", "Настройки"],
    Language => ["Language", "Sprache", "Nyelv", "Langue", "Lingua", "Язык"],
    About => ["About", "Über NeonTrace", "Névjegy", "À propos", "Informazioni", "О программе"],
    Demo => ["Offline demonstration", "Offline-Demonstration", "Offline bemutató", "Démonstration hors ligne", "Dimostrazione offline", "Автономная демонстрация"],
    DemoWarning => ["Fictional observations · no network probes", "Fiktive Beobachtungen · keine Netzwerkproben", "Kitalált megfigyelések · nincs hálózati mérés", "Observations fictives · aucune sonde réseau", "Osservazioni fittizie · nessuna sonda di rete", "Вымышленные наблюдения · без сетевых запросов"],
    Live => ["Live measurement", "Live-Messung", "Élő mérés", "Mesure en direct", "Misurazione in tempo reale", "Измерение в реальном времени"],
    ExitDemo => ["Return to live mode", "Zum Live-Modus", "Vissza az élő módhoz", "Revenir au mode réel", "Torna alla modalità reale", "Вернуться к измерениям"],
    DemoLabel => ["DEMO", "DEMO", "BEMUTATÓ", "DÉMO", "DEMO", "ДЕМО"],
    ObservedRoute => ["Observed probe route · approximate locations", "Beobachtete Proberoute · ungefähre Orte", "Megfigyelt mérési útvonal · becsült helyek", "Itinéraire observé · positions approximatives", "Percorso osservato · posizioni approssimative", "Наблюдаемый маршрут · примерные местоположения"],
    Schematic => ["Schematic arcs · outbound animation: 8 seconds", "Schematische Bögen · ausgehende Animation: 8 Sekunden", "Sematikus ívek · kimenő animáció: 8 másodperc", "Arcs schématiques · animation aller : 8 secondes", "Archi schematici · animazione in uscita: 8 secondi", "Схематичные дуги · анимация к цели: 8 секунд"],
    Geography => ["World geography", "Weltkarte", "Világtérkép", "Carte du monde", "Mappa mondiale", "Карта мира"],
    Coastline => ["Coastline", "Küstenlinie", "Partvonal", "Littoral", "Costa", "Береговая линия"],
    Outbound => ["Outbound route", "Ausgehende Route", "Kimenő útvonal", "Trajet aller", "Percorso in uscita", "Маршрут к цели"],
    LocationUnknown => ["Location unknown", "Ort unbekannt", "Ismeretlen hely", "Position inconnue", "Posizione sconosciuta", "Местоположение неизвестно"],
    Unknown => ["Unknown", "Unbekannt", "Ismeretlen", "Inconnu", "Sconosciuto", "Неизвестно"],
    Unlocated => ["Unlocated hops", "Hops ohne Ort", "Helyadat nélküli ugrások", "Sauts non localisés", "Hop senza posizione", "Узлы без местоположения"],
    CountryOnly => ["Country estimate only", "Nur geschätztes Land", "Csak becsült ország", "Estimation du pays uniquement", "Solo stima del paese", "Известна только примерная страна"],
    OutsideMap => ["Outside map coverage", "Außerhalb der Kartenabdeckung", "A térkép területén kívül", "Hors de la couverture cartographique", "Fuori dalla copertura della mappa", "За пределами карты"],
    Endpoint => ["Endpoint", "Endpunkt", "Végpont", "Point de terminaison", "Endpoint", "Конечный узел"],
    Origin => ["Public egress", "Öffentlicher Netzausgang", "Nyilvános hálózati kijárat", "Sortie réseau publique", "Uscita di rete pubblica", "Публичный сетевой выход"],
    EstimatedOrigin => ["Estimated network egress (ISP/VPN), not the physical location of your device.", "Geschätzter Netzausgang (Internetanbieter/VPN), nicht der physische Standort Ihres Geräts.", "Becsült hálózati kijárat (internetszolgáltató/VPN), nem az eszköz fizikai helye.", "Sortie réseau estimée (FAI/VPN), pas la position physique de votre appareil.", "Uscita di rete stimata (provider/VPN), non la posizione fisica del dispositivo.", "Примерное местоположение сетевого выхода (провайдер/VPN), а не самого устройства."],
    YourPc => ["Your PC", "Ihr PC", "Saját számítógép", "Votre PC", "Il tuo PC", "Ваш компьютер"],
    DestinationResponse => ["Destination response", "Zielantwort", "Célpont válaszideje", "Réponse de la cible", "Risposta della destinazione", "Ответ конечного узла"],
    CurrentRtt => ["Current RTT", "Aktuelle RTT", "Aktuális RTT", "RTT actuel", "RTT attuale", "Текущее RTT"],
    Minimum => ["Minimum", "Minimum", "Minimum", "Minimum", "Minimo", "Минимум"],
    Average => ["Average", "Durchschnitt", "Átlag", "Moyenne", "Media", "Среднее"],
    Maximum => ["Maximum", "Maximum", "Maximum", "Maximum", "Massimo", "Максимум"],
    Replies => ["Replies", "Antworten", "Válaszok", "Réponses", "Risposte", "Ответы"],
    Nonresponses => ["Nonresponses", "Ohne Antwort", "Válasz nélkül", "Sans réponse", "Senza risposta", "Без ответа"],
    SampleWindow => ["Statistics cover the latest 3,600 endpoint probes.", "Statistiken umfassen die letzten 3.600 Zielproben.", "A statisztikák a legutóbbi 3600 végpontmérést fedik le.", "Les statistiques couvrent les 3 600 dernières sondes de la cible.", "Le statistiche coprono le ultime 3.600 sonde della destinazione.", "Статистика охватывает последние 3600 проверок конечного узла."],
    Samples => ["Samples", "Messwerte", "Minták", "Échantillons", "Campioni", "Измерения"],
    Elapsed => ["Elapsed", "Laufzeit", "Eltelt idő", "Durée écoulée", "Tempo trascorso", "Прошло времени"],
    RouteAge => ["Route age", "Alter der Route", "Útvonal kora", "Âge du tracé", "Età del percorso", "Возраст маршрута"],
    RouteSnapshot => ["Route snapshot", "Routenaufnahme", "Útvonalpillanatkép", "Instantané du tracé", "Istantanea del percorso", "Снимок маршрута"],
    Hop => ["Hop", "Hop", "Ugrás", "Saut", "Hop", "Узел"],
    Hops => ["Hops", "Hops", "Ugrások", "Sauts", "Hop", "Узлы"],
    Address => ["Address", "Adresse", "Cím", "Adresse", "Indirizzo", "Адрес"],
    AddressFamily => ["Address family", "Adressfamilie", "Címcsalád", "Famille d’adresses", "Famiglia di indirizzi", "Семейство адресов"],
    SelectedAddress => ["Selected address", "Ausgewählte Adresse", "Kiválasztott cím", "Adresse sélectionnée", "Indirizzo selezionato", "Выбранный адрес"],
    Network => ["Network", "Netzwerk", "Hálózat", "Réseau", "Rete", "Сеть"],
    Location => ["Location", "Ort", "Hely", "Position", "Posizione", "Местоположение"],
    Protocol => ["Protocol", "Protokoll", "Protokoll", "Protocole", "Protocollo", "Протокол"],
    Timestamp => ["Timestamp", "Zeitstempel", "Időbélyeg", "Horodatage", "Data e ora", "Время наблюдения"],
    AlternateReplies => ["Alternate observations", "Alternative Beobachtungen", "Alternatív megfigyelések", "Observations alternatives", "Osservazioni alternative", "Альтернативные наблюдения"],
    HopTimingNote => ["Hop RTT is measured from this computer; it is not a link delay.", "Hop-RTT wird ab diesem Computer gemessen; sie ist keine Verbindungsverzögerung.", "Az ugrás RTT-je ettől a géptől mért oda-vissza idő, nem a szakasz késleltetése.", "Le RTT d’un saut est mesuré depuis cet ordinateur ; ce n’est pas le délai d’un lien.", "L’RTT di un hop è misurato da questo computer; non è il ritardo di un collegamento.", "RTT узла измеряется от этого компьютера и не является задержкой отдельного канала."],
    RouteCaveat => ["The displayed route is a snapshot. Later pings may take another route.", "Die Route ist eine Momentaufnahme. Spätere Pings können anders verlaufen.", "A megjelenített útvonal pillanatkép. A későbbi pingek más útvonalon haladhatnak.", "Le tracé est un instantané. Les pings suivants peuvent emprunter un autre chemin.", "Il percorso è un’istantanea. I ping successivi possono seguire un altro percorso.", "Маршрут показан на момент трассировки. Последующие ping могут идти другим путём."],
    TcpCheck => ["Separate TCP 443 check", "Separate TCP-443-Prüfung", "Külön TCP 443 ellenőrzés", "Test TCP 443 distinct", "Verifica TCP 443 separata", "Отдельная проверка TCP 443"],
    TcpNote => ["Connection timing only; not ICMP or website health. Applies on Start.", "Nur Verbindungsdauer; kein ICMP- oder Website-Test. Gilt ab dem nächsten Start.", "Csak kapcsolódási idő; nem ICMP- vagy webhelyállapot-mérés. Indításkor lép életbe.", "Durée de connexion uniquement ; ni ICMP ni test de santé du site. Appliqué au démarrage.", "Solo tempo di connessione; non è ICMP né un test del sito. Si applica all’avvio.", "Только время соединения; это не ICMP и не проверка сайта. Применяется при запуске измерений."],
    Connected => ["Connected", "Verbunden", "Kapcsolódva", "Connecté", "Connesso", "Подключено"],
    ConnectionFailed => ["Connection failed", "Verbindung fehlgeschlagen", "Sikertelen kapcsolódás", "Échec de connexion", "Connessione non riuscita", "Не удалось подключиться"],
    NotChecked => ["Not checked", "Nicht geprüft", "Nincs ellenőrizve", "Non testé", "Non verificato", "Не проверено"],
    Idle => ["Ready · measurements stopped", "Bereit · Messungen gestoppt", "Kész · mérések leállítva", "Prêt · mesures arrêtées", "Pronto · misurazioni ferme", "Готово · измерения остановлены"],
    Resolving => ["Resolving target…", "Ziel wird aufgelöst…", "Célpont feloldása…", "Résolution de la cible…", "Risoluzione della destinazione…", "Определение адреса цели…"],
    Measuring => ["Measuring", "Messung läuft", "Mérés folyamatban", "Mesure en cours", "Misurazione in corso", "Измерение"],
    Tracing => ["Discovering route…", "Route wird ermittelt…", "Útvonal felderítése…", "Découverte du tracé…", "Rilevamento del percorso…", "Определение маршрута…"],
    PartialRoute => ["Partial route", "Unvollständige Route", "Részleges útvonal", "Tracé partiel", "Percorso parziale", "Неполный маршрут"],
    CompleteRoute => ["Trace complete", "Routenermittlung abgeschlossen", "Útvonalmérés kész", "Traçage terminé", "Tracciamento completato", "Трассировка завершена"],
    NetworkChanged => ["Network changed · measurements stopped; start a fresh session.", "Netzwerk geändert · Messungen gestoppt; neue Sitzung starten.", "A hálózat megváltozott · a mérés leállt; indítson új munkamenetet.", "Réseau modifié · mesures arrêtées ; démarrez une nouvelle session.", "Rete cambiata · misurazioni ferme; avvia una nuova sessione.", "Сеть изменилась · измерения остановлены; начните новый сеанс."],
    RouteChanged => ["New route snapshot", "Neue Routenaufnahme", "Új útvonalpillanatkép", "Nouvel instantané du tracé", "Nuova istantanea del percorso", "Новый снимок маршрута"],
    Stopped => ["Stopped", "Gestoppt", "Leállítva", "Arrêté", "Fermato", "Остановлено"],
    NoReply => ["No reply", "Keine Antwort", "Nincs válasz", "Aucune réponse", "Nessuna risposta", "Нет ответа"],
    Timeout => ["Timed out", "Zeitüberschreitung", "Időtúllépés", "Délai dépassé", "Tempo scaduto", "Время ожидания истекло"],
    Unreachable => ["Unreachable response", "Nicht-erreichbar-Antwort", "Elérhetetlen válasz", "Réponse inaccessible", "Risposta non raggiungibile", "Ответ о недоступности"],
    NoData => ["No observations yet", "Noch keine Beobachtungen", "Még nincs megfigyelés", "Aucune observation", "Nessuna osservazione", "Наблюдений пока нет"],
    NoRoute => ["No route snapshot available", "Keine Routenaufnahme verfügbar", "Nincs útvonalpillanatkép", "Aucun instantané du tracé", "Nessuna istantanea disponibile", "Снимок маршрута отсутствует"],
    NoReplyCaveat => ["An intermediate nonresponse does not prove lost forwarded traffic.", "Eine fehlende Zwischenantwort beweist keinen Verlust weitergeleiteter Pakete.", "Egy köztes válasz hiánya nem bizonyítja a továbbított forgalom elvesztését.", "L’absence de réponse intermédiaire ne prouve pas une perte du trafic transmis.", "Una mancata risposta intermedia non dimostra perdita del traffico inoltrato.", "Отсутствие ответа промежуточного узла не доказывает потерю транзитного трафика."],
    PermissionDenied => ["ICMP probe permission unavailable. The separate TCP 443 check remains available.", "Keine Berechtigung für ICMP-Proben. Die separate TCP-443-Prüfung bleibt verfügbar.", "Nincs jogosultság ICMP-méréshez. A külön TCP 443 ellenőrzés továbbra is elérhető.", "Autorisation ICMP indisponible. Le test TCP 443 distinct reste disponible.", "Permesso ICMP non disponibile. La verifica TCP 443 separata resta disponibile.", "Недостаточно прав для ICMP. Отдельная проверка TCP 443 остаётся доступной."],
    TraceUnavailable => ["Tracing unavailable; endpoint measurements remain separate.", "Routenermittlung nicht verfügbar; Endpunktmessungen bleiben getrennt.", "Az útvonalmérés nem érhető el; a végpontmérés ettől független.", "Traçage indisponible ; les mesures de la cible restent distinctes.", "Tracciamento non disponibile; le misure dell’endpoint restano separate.", "Трассировка недоступна; измерения конечного узла выполняются отдельно."],
    PermissionHelp => ["Keep the GUI unprivileged. See the platform probe requirements in the notes.", "Die Oberfläche ohne erhöhte Rechte ausführen. Plattformanforderungen stehen in den Hinweisen.", "A felületet emelt jogosultság nélkül futtassa. A mérési követelmények a jegyzetekben találhatók.", "Exécutez l’interface sans privilèges. Consultez les exigences des sondes dans les notes.", "Esegui l’interfaccia senza privilegi. Consulta i requisiti delle sonde nelle note.", "Запускайте интерфейс без повышенных прав. Требования к зондированию приведены в примечаниях."],
    InvalidTarget => ["Enter a valid hostname, IPv4/IPv6 address or URL.", "Gültigen Hostnamen, IPv4-/IPv6-Adresse oder URL eingeben.", "Érvényes gépnevet, IPv4-/IPv6-címet vagy URL-t adjon meg.", "Saisissez un nom d’hôte, une adresse IPv4/IPv6 ou une URL valide.", "Inserisci un nome host, indirizzo IPv4/IPv6 o URL valido.", "Введите допустимое имя узла, адрес IPv4/IPv6 или URL."],
    EmptyTarget => ["Enter a target.", "Ein Ziel eingeben.", "Adjon meg egy célpontot.", "Saisissez une cible.", "Inserisci una destinazione.", "Введите цель."],
    TargetCountError => ["Enter between 1 and 10 targets.", "Zwischen 1 und 10 Ziele eingeben.", "1 és 10 közötti számú célpontot adjon meg.", "Saisissez entre 1 et 10 cibles.", "Inserisci da 1 a 10 destinazioni.", "Введите от 1 до 10 целей."],
    DuplicatesRemoved => ["Exact duplicate targets were removed.", "Identische Ziele wurden entfernt.", "Az azonos célpontokat eltávolítottuk.", "Les cibles identiques ont été supprimées.", "Le destinazioni duplicate sono state rimosse.", "Точные дубликаты целей удалены."],
    ResolutionFailed => ["Target resolution failed.", "Zielauflösung fehlgeschlagen.", "A célpont feloldása sikertelen.", "Échec de la résolution de la cible.", "Risoluzione della destinazione non riuscita.", "Не удалось определить адрес цели."],
    NoAddresses => ["No usable IP address found.", "Keine verwendbare IP-Adresse gefunden.", "Nincs használható IP-cím.", "Aucune adresse IP utilisable trouvée.", "Nessun indirizzo IP utilizzabile trovato.", "Подходящий IP-адрес не найден."],
    NetworkError => ["Network operation failed.", "Netzwerkvorgang fehlgeschlagen.", "A hálózati művelet sikertelen.", "Échec de l’opération réseau.", "Operazione di rete non riuscita.", "Ошибка сетевой операции."],
    TechnicalDetails => ["Technical details", "Technische Details", "Műszaki részletek", "Détails techniques", "Dettagli tecnici", "Технические сведения"],
    ExtractedHost => ["URL host: {host} · page paths are not tested", "URL-Host: {host} · Seitenpfade werden nicht geprüft", "URL-gépnév: {host} · az oldal útvonala nincs tesztelve", "Hôte de l’URL : {host} · les chemins des pages ne sont pas testés", "Host dell’URL: {host} · i percorsi delle pagine non vengono verificati", "Узел URL: {host} · пути страниц не проверяются"],
    TargetNumber => ["Target {number}", "Ziel {number}", "{number}. célpont", "Cible {number}", "Destinazione {number}", "Цель {number}"],
    CountTargets => ["{count} targets", "{count} Ziele", "{count} célpont", "{count} cibles", "{count} destinazioni", "Целей: {count}"],
    Inspecting => ["Inspecting target {number}", "Ziel {number} wird angezeigt", "Megjelenítve: {number}. célpont", "Détails de la cible {number}", "Dettagli destinazione {number}", "Просмотр цели {number}"],
    Appearance => ["Appearance", "Darstellung", "Megjelenés", "Apparence", "Aspetto", "Оформление"],
    SystemTheme => ["Colours follow the operating system.", "Farben folgen dem Betriebssystem.", "A színek az operációs rendszert követik.", "Les couleurs suivent le système d’exploitation.", "I colori seguono il sistema operativo.", "Цвета определяются операционной системой."],
    CountryBorders => ["Country borders", "Ländergrenzen", "Országhatárok", "Frontières des pays", "Confini nazionali", "Границы стран"],
    LocationLabels => ["Location labels", "Ortsbeschriftungen", "Helyfeliratok", "Noms des lieux", "Etichette delle posizioni", "Названия мест"],
    ReducedMotion => ["Reduced motion", "Weniger Bewegung", "Csökkentett mozgás", "Animations réduites", "Movimento ridotto", "Уменьшение движения"],
    ReducedEffects => ["Reduced rendering effects", "Reduzierte Darstellungseffekte", "Csökkentett megjelenítési effektek", "Effets visuels réduits", "Effetti grafici ridotti", "Упрощённые графические эффекты"],
    History => ["History", "Verlauf", "Előzmények", "Historique", "Cronologia", "История"],
    EnableHistory => ["Save local history", "Lokalen Verlauf speichern", "Helyi előzmények mentése", "Enregistrer l’historique local", "Salva cronologia locale", "Сохранять локальную историю"],
    HistoryDisabled => ["History is disabled. Targets remain in memory only.", "Verlauf deaktiviert. Ziele bleiben nur im Arbeitsspeicher.", "Az előzmények ki vannak kapcsolva. A célpontok csak a memóriában maradnak.", "Historique désactivé. Les cibles restent uniquement en mémoire.", "Cronologia disattivata. Le destinazioni rimangono solo in memoria.", "История отключена. Цели хранятся только в памяти."],
    ClearHistory => ["Clear history", "Verlauf löschen", "Előzmények törlése", "Effacer l’historique", "Cancella cronologia", "Очистить историю"],
    DeleteRecord => ["Delete record", "Eintrag löschen", "Bejegyzés törlése", "Supprimer l’enregistrement", "Elimina voce", "Удалить запись"],
    EmptyHistory => ["No saved observations", "Keine gespeicherten Beobachtungen", "Nincs mentett megfigyelés", "Aucune observation enregistrée", "Nessuna osservazione salvata", "Сохранённых наблюдений нет"],
    SavedResult => ["Saved observation · not live", "Gespeicherte Beobachtung · nicht live", "Mentett megfigyelés · nem élő", "Observation enregistrée · hors direct", "Osservazione salvata · non in tempo reale", "Сохранённое наблюдение · не текущее"],
    HistoryRetention => ["The newest 100 sessions are retained.", "Die neuesten 100 Sitzungen werden gespeichert.", "A legutóbbi 100 munkamenetet őrizzük meg.", "Les 100 sessions les plus récentes sont conservées.", "Vengono conservate le 100 sessioni più recenti.", "Сохраняются 100 последних сеансов."],
    StorageError => ["Could not save local data.", "Lokale Daten konnten nicht gespeichert werden.", "A helyi adatok mentése sikertelen.", "Impossible d’enregistrer les données locales.", "Impossibile salvare i dati locali.", "Не удалось сохранить локальные данные."],
    Export => ["Export", "Exportieren", "Exportálás", "Exporter", "Esporta", "Экспорт"],
    ExportPng => ["Export PNG", "PNG exportieren", "PNG exportálása", "Exporter en PNG", "Esporta PNG", "Экспорт PNG"],
    ExportJson => ["Export JSON", "JSON exportieren", "JSON exportálása", "Exporter en JSON", "Esporta JSON", "Экспорт JSON"],
    ExportCsv => ["Export CSV", "CSV exportieren", "CSV exportálása", "Exporter en CSV", "Esporta CSV", "Экспорт CSV"],
    ExportSaved => ["Export saved: {path}", "Export gespeichert: {path}", "Export mentve: {path}", "Export enregistré : {path}", "Esportazione salvata: {path}", "Экспорт сохранён: {path}"],
    ExportSucceeded => ["Export saved", "Export gespeichert", "Export mentve", "Export enregistré", "Esportazione salvata", "Экспорт сохранён"],
    ExportFailed => ["Export failed.", "Export fehlgeschlagen.", "Sikertelen exportálás.", "Échec de l’export.", "Esportazione non riuscita.", "Не удалось экспортировать."],
    ExportCancelled => ["Export cancelled", "Export abgebrochen", "Exportálás megszakítva", "Export annulé", "Esportazione annullata", "Экспорт отменён"],
    NothingToExport => ["No observations to export.", "Keine Beobachtungen zum Exportieren.", "Nincs exportálható megfigyelés.", "Aucune observation à exporter.", "Nessuna osservazione da esportare.", "Нет наблюдений для экспорта."],
    Measurements => ["Measurements", "Messungen", "Mérések", "Mesures", "Misurazioni", "Измерения"],
    Approximation => ["Locations are estimates; arcs do not represent cables.", "Orte sind Schätzungen; Bögen stellen keine Kabel dar.", "A helyek becslések; az ívek nem kábeleket ábrázolnak.", "Les positions sont estimées ; les arcs ne représentent pas des câbles.", "Le posizioni sono stime; gli archi non rappresentano cavi.", "Местоположения приблизительны; дуги не показывают кабели."],
    DataSource => ["Data source", "Datenquelle", "Adatforrás", "Source des données", "Fonte dei dati", "Источник данных"],
    DataEdition => ["Database edition", "Datenbankausgabe", "Adatbázis-kiadás", "Édition de la base", "Edizione del database", "Версия базы данных"],
    LocationData => ["Location databases", "Ortsdatenbanken", "Helyadatbázisok", "Bases de localisation", "Database di geolocalizzazione", "Базы геолокации"],
    DownloadOnFirstUse => ["City and ASN data download at first live use.", "Stadt- und ASN-Daten werden bei der ersten Live-Nutzung geladen.", "A város- és ASN-adatok az első élő használatkor töltődnek le.", "Les données de ville et d’ASN sont téléchargées à la première mesure réelle.", "I dati città e ASN vengono scaricati al primo utilizzo reale.", "Данные городов и ASN загружаются при первом реальном измерении."],
    CityDatabase => ["City", "Städte", "Város", "Villes", "Città", "Города"],
    AsnDatabase => ["ASN", "ASN", "ASN", "ASN", "ASN", "ASN"],
    DownloadingCity => ["Downloading City database…", "Stadtdatenbank wird geladen…", "Városadatbázis letöltése…", "Téléchargement de la base des villes…", "Download del database città…", "Загрузка базы городов…"],
    DownloadingAsn => ["Downloading ASN database…", "ASN-Datenbank wird geladen…", "ASN-adatbázis letöltése…", "Téléchargement de la base ASN…", "Download del database ASN…", "Загрузка базы ASN…"],
    ValidatingDatabase => ["Validating downloaded database…", "Geladene Datenbank wird geprüft…", "Letöltött adatbázis ellenőrzése…", "Vérification de la base téléchargée…", "Verifica del database scaricato…", "Проверка загруженной базы…"],
    DatabaseReady => ["Cached databases ready", "Zwischengespeicherte Datenbanken bereit", "Gyorsítótárazott adatbázisok készen", "Bases en cache prêtes", "Database in cache pronti", "Сохранённые базы готовы"],
    DatabaseMissing => ["Location data unavailable; probes can continue.", "Ortsdaten fehlen; Proben können fortgesetzt werden.", "Nincs helyadat; a mérések folytathatók.", "Localisation indisponible ; les sondes peuvent continuer.", "Dati posizione non disponibili; le sonde possono continuare.", "Данные местоположений недоступны; измерения могут продолжаться."],
    DatabaseFailed => ["Database update failed; the last valid cache is retained.", "Datenbankaktualisierung fehlgeschlagen; letzter gültiger Cache bleibt erhalten.", "Az adatbázis-frissítés sikertelen; az utolsó érvényes példány megmarad.", "Échec de mise à jour ; le dernier cache valide est conservé.", "Aggiornamento non riuscito; l’ultima cache valida viene conservata.", "Обновление базы не удалось; последняя исправная копия сохранена."],
    InsufficientSpace => ["Insufficient space for database download.", "Nicht genug Speicherplatz für die Datenbank.", "Nincs elegendő hely az adatbázis letöltéséhez.", "Espace insuffisant pour télécharger la base.", "Spazio insufficiente per il download del database.", "Недостаточно места для загрузки базы."],
    InvalidDatabase => ["Downloaded database is invalid.", "Geladene Datenbank ist ungültig.", "A letöltött adatbázis érvénytelen.", "La base téléchargée est invalide.", "Il database scaricato non è valido.", "Загруженная база недействительна."],
    DiscoveringOrigin => ["Discovering public egress…", "Öffentlicher Netzausgang wird ermittelt…", "Nyilvános kijárat felderítése…", "Recherche de la sortie réseau publique…", "Rilevamento dell’uscita di rete pubblica…", "Определение публичного сетевого выхода…"],
    OriginUnavailable => ["Public egress unavailable; no origin is invented.", "Öffentlicher Netzausgang unbekannt; kein Ursprung wird erfunden.", "A nyilvános kijárat ismeretlen; nem adunk meg kitalált kiindulópontot.", "Sortie publique indisponible ; aucune origine n’est inventée.", "Uscita pubblica non disponibile; non viene inventata un’origine.", "Публичный выход недоступен; начальная точка не выдумывается."],
    Retry => ["Retry", "Erneut versuchen", "Újrapróbálás", "Réessayer", "Riprova", "Повторить"],
    Refresh => ["Refresh databases", "Datenbanken aktualisieren", "Adatbázisok frissítése", "Actualiser les bases", "Aggiorna database", "Обновить базы"],
    CheckingUpdates => ["Checking monthly database editions…", "Monatliche Datenbankausgaben werden geprüft…", "Havi adatbázis-kiadások ellenőrzése…", "Vérification des éditions mensuelles…", "Verifica delle edizioni mensili…", "Проверка ежемесячных версий баз…"],
    CacheAge => ["Cache age", "Cache-Alter", "Gyorsítótár kora", "Âge du cache", "Età della cache", "Возраст сохранённой копии"],
    Privacy => ["Privacy", "Datenschutz", "Adatvédelem", "Confidentialité", "Privacy", "Конфиденциальность"],
    PrivacyNote => ["No telemetry. Live mode uses DNS, probes, ipify and DB-IP downloads. Hop lookups stay local.", "Keine Telemetrie. Live-Modus nutzt DNS, Proben, ipify und DB-IP-Downloads. Hop-Abfragen bleiben lokal.", "Nincs telemetria. Az élő mód DNS-t, méréseket, ipify-t és DB-IP-letöltéseket használ. Az ugrások keresése helyben történik.", "Aucune télémétrie. Le mode réel utilise DNS, sondes, ipify et téléchargements DB-IP. Les recherches de sauts restent locales.", "Nessuna telemetria. La modalità reale usa DNS, sonde, ipify e download DB-IP. Le ricerche degli hop restano locali.", "Без телеметрии. Реальный режим использует DNS, зонды, ipify и загрузки DB-IP. Поиск узлов выполняется локально."],
    Credits => ["Licenses and data credits", "Lizenzen und Datenquellen", "Licencek és adatforrások", "Licences et sources", "Licenze e fonti dei dati", "Лицензии и источники данных"],
    MapCredit => ["Map: Simplemaps · Robinson projection · Antarctica not included", "Karte: Simplemaps · Robinson-Projektion · ohne Antarktis", "Térkép: Simplemaps · Robinson-vetület · Antarktisz nélkül", "Carte : Simplemaps · projection Robinson · sans Antarctique", "Mappa: Simplemaps · proiezione Robinson · Antartide esclusa", "Карта: Simplemaps · проекция Робинсона · без Антарктиды"],
    GeoCredit => ["IP Geolocation by DB-IP · CC BY 4.0", "IP-Geolokalisierung von DB-IP · CC BY 4.0", "IP-helymeghatározás: DB-IP · CC BY 4.0", "Géolocalisation IP par DB-IP · CC BY 4.0", "Geolocalizzazione IP di DB-IP · CC BY 4.0", "IP-геолокация: DB-IP · CC BY 4.0"],
    Copy => ["Copy", "Kopieren", "Másolás", "Copier", "Copia", "Копировать"],
    Copied => ["Copied", "Kopiert", "Másolva", "Copié", "Copiato", "Скопировано"],
    On => ["On", "Ein", "Be", "Activé", "Attivo", "Включено"],
    Off => ["Off", "Aus", "Ki", "Désactivé", "Disattivo", "Выключено"],
    SecondsValue => ["{value} s", "{value} s", "{value} mp", "{value} s", "{value} s", "{value} с"],
    DaysValue => ["{value} days", "{value} Tage", "{value} nap", "{value} jours", "{value} giorni", "Дней: {value}"],
    AnimationProgress => ["Presentation progress", "Darstellungsfortschritt", "Megjelenítés állapota", "Progression de l’animation", "Avanzamento della presentazione", "Ход анимации"],
    NotDeliveryProgress => ["Animation progress is not network delivery progress.", "Animationsfortschritt ist kein Fortschritt der Netzwerkübertragung.", "Az animáció állapota nem a hálózati kézbesítés állapota.", "La progression de l’animation ne mesure pas la livraison réseau.", "L’avanzamento dell’animazione non indica la consegna di rete.", "Ход анимации не отражает доставку данных по сети."],
    Complete => ["Complete", "Abgeschlossen", "Kész", "Terminé", "Completato", "Завершено"],
    Unavailable => ["Unavailable", "Nicht verfügbar", "Nem érhető el", "Indisponible", "Non disponibile", "Недоступно"],
    Selected => ["Selected", "Ausgewählt", "Kiválasztva", "Sélectionné", "Selezionato", "Выбрано"],
    Error => ["Error", "Fehler", "Hiba", "Erreur", "Errore", "Ошибка"],
    Details => ["Details", "Details", "Részletek", "Détails", "Dettagli", "Подробности"],
    CloseDetails => ["Close details", "Details schließen", "Részletek bezárása", "Fermer les détails", "Chiudi dettagli", "Закрыть подробности"],
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeSet;
    fn placeholders(s: &str) -> BTreeSet<&str> {
        s.split('{')
            .skip(1)
            .filter_map(|rest| rest.split_once('}').map(|(name, _)| name))
            .collect()
    }
    #[test]
    fn every_locale_has_every_key_and_matching_parameters() {
        for &key in Key::ALL {
            let translations = key.translations();
            let expected = placeholders(translations[0]);
            for lang in Language::ALL {
                let value = lang.text(key);
                assert!(!value.trim().is_empty(), "{lang:?}/{key:?} is empty");
                assert_eq!(
                    placeholders(value),
                    expected,
                    "{lang:?}/{key:?}: parameters differ"
                );
                assert!(
                    !value.contains('\u{fffd}'),
                    "{lang:?}/{key:?}: replacement glyph"
                );
            }
        }
    }
    #[test]
    fn english_is_default_and_only_six_languages_exist() {
        assert_eq!(Language::default(), Language::En);
        assert_eq!(Language::ALL.len(), 6);
        for language in Language::ALL {
            assert_eq!(Language::from_code(language.code()), language);
        }
    }
    #[test]
    fn named_parameters_follow_local_order() {
        assert_eq!(
            Language::Hu.format(Key::TargetNumber, &[("number", "3")]),
            "3. célpont"
        );
        assert_eq!(
            Language::Fr.format(Key::TargetNumber, &[("number", "3")]),
            "Cible 3"
        );
    }
}
