//! Preferences come from either the user or are programmatically set by the AT.
//! The either source can set any preference, but users and AT typically set different preferences.
//!
//! User prefs are read in from a YAML file (prefs.yaml). The can be written by hand.
//! In the future, there will hopefully be a nice UI that writes out the YAML file.
//!
//! AT prefs are set via the API given in the [crate::interface] module.
//! These in turn call [`PreferenceManager::set_api_string_pref`] and [`PreferenceManager::set_api_float_pref`].
//! Ultimately, user and api prefs are stored in a hashmap.
//!
//! Preferences can be found in a few places:
//! 1. Language-independent prefs found in the Rules dir
//! 2. Language-specific prefs
//! 3. Language-region-specific prefs
//! If there are multiple definitions, the later ones overwrite the former ones.
//! This means that region-specific variants will overwrite more general variants.
//!
//! Note: there are a number of public 'get_xxx' functions that really are meant to be public only to the [crate::speech] module as speech needs access
//! to the preferences to generate the speech.

use yaml_rust::{Yaml, YamlLoader};
use crate::pretty_print::yaml_to_string;
use crate::tts::TTS;
extern crate dirs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime};
use std::env;
use crate::speech::{as_str_checked, print_errors};
use std::collections::HashMap;


// Preferences are recorded here
/// Preferences are stored in a HashMap. It maps the name of the pref (a String) to its value (stored as YAML string/float)
pub type PreferenceHashMap = HashMap<String, Yaml>;
#[derive(Debug, Clone)]

struct Preferences {
    prefs: PreferenceHashMap        // FIX: pub so can get at iterator, should add iterator to Preferences instead
}

impl Preferences{
    // default values needed in case nothing else gets set 
    fn user_defaults() -> Preferences {
        let mut prefs = PreferenceHashMap::with_capacity(7);
        prefs.insert("Language".to_string(), Yaml::String("en".to_string()));
        prefs.insert("SpeechStyle".to_string(), Yaml::String("ClearSpeak".to_string()));
        prefs.insert("Verbosity".to_string(), Yaml::String("medium".to_string()));
        prefs.insert("Blind".to_string(), Yaml::Boolean(true));
        prefs.insert("NavigationMode".to_string(), Yaml::String("enhanced".to_string()));
        prefs.insert("NavigationSpeech".to_string(), Yaml::String("read".to_string()));
        prefs.insert("Blind".to_string(), Yaml::Boolean(true));

        return Preferences{ prefs };
    }

    // default values needed in case nothing else gets set 
    fn api_defaults() -> Preferences {
        let mut prefs = PreferenceHashMap::with_capacity(7);
        prefs.insert("TTS".to_string(), Yaml::String("none".to_string()));
        prefs.insert("Pitch".to_string(), Yaml::Real("1.0".to_string()));
        prefs.insert("Rate".to_string(), Yaml::Real("180.0".to_string()));
        prefs.insert("Volume".to_string(), Yaml::Real("100.0".to_string()));
        prefs.insert("Voice".to_string(), Yaml::String("none".to_string()));
        prefs.insert("Gender".to_string(), Yaml::String("none".to_string()));
        return Preferences{ prefs };
    }

    // Before we can get the other files, we need the preferences.
    // To get them we need to read pref files, so the pref file reading is different than the other files
    fn from_file(rules_dir: &PathBuf) -> (Preferences, FileAndTime) {
        let files = Preferences::get_prefs_file_and_time(rules_dir);
        return DEFAULT_USER_PREFERENCES.with(|defaults| {
            let mut system_prefs = Preferences::read_file(&files.files[0], defaults.clone());
            system_prefs = Preferences::read_file(&files.files[1], system_prefs);
            return (system_prefs, files);
        });
    }

    fn get_prefs_file_and_time(rules_dir: &PathBuf) -> FileAndTime {
        let mut system_prefs_file = rules_dir.clone();
        system_prefs_file.push("prefs.yaml");

        let mut result: [Option<PathBuf>; 3] = [None, None, None];
        if system_prefs_file.is_file() {
            result[0] = Some( system_prefs_file );
        } else {
            eprintln!("Couldn't open file {}.\nUsing fallback defaults which may be inappropriate.",
                        system_prefs_file.to_str().unwrap());
        }

        let user_dir = dirs::config_dir();
        if let Some(mut user_prefs_file) = user_dir {
            user_prefs_file.push("prefs.yaml");
            if user_prefs_file.is_file() {
                result[1] = Some( user_prefs_file );
            }            
        }

        return FileAndTime {
            time: Some( SystemTime::now() ),
            files: result
        }
    }

    fn read_file(file: &Option<PathBuf>, base_prefs: Preferences) -> Preferences {
        use std::fs;
        let unwrapped_file;
        match file {
            None => return base_prefs,
            Some(f) => unwrapped_file = f,
        };

        let file_name = unwrapped_file.to_str().unwrap();
        let docs;
        match fs::read_to_string(unwrapped_file) {
            Err(e) => {
                eprint!("Couldn't read file {}\n{}", file_name, e);
                return base_prefs;
            }
            Ok( file_contents) => {
                match YamlLoader::load_from_str(&file_contents) {
                    Err(e) => {
                        eprintln!("Yaml parse error ('{}') in file {}.\nUsing fallback defaults which may be inappropriate.",
                                    e, file_name);
                        return base_prefs;
                    },
                    Ok(d) => docs = d,
                }

            }
        }
        if docs.len() != 1 {
            eprintln!("Yaml error in file {}.\nFound {} 'documents' -- should only be 1.",
                        file_name, docs.len());
            return base_prefs;
        }

        let doc = &docs[0];
        verify_keys(doc, "Speech", file_name);
        verify_keys(doc, "Navigation", file_name);
        verify_keys(doc, "Braille", file_name);

        return DEFAULT_USER_PREFERENCES.with(|defaults| {
            let prefs = &mut defaults.prefs.clone(); // ensure basic key/values exist
            add_prefs(prefs, &doc["Speech"], "", file_name);
            add_prefs(prefs, &doc["Navigation"], "", file_name);
            add_prefs(prefs, &doc["Braille"], "", file_name);
            return Preferences{ prefs: prefs.to_owned() };
        });



        fn verify_keys(dict: &Yaml, key: &str, file_name: &str) {
            let prefs = &dict[key];
            if prefs.is_badvalue() {
                eprintln!("Yaml error in file {}.\nDidn't find '{}' key.", file_name, key);
            }
            if prefs.as_hash().is_none() {
                eprintln!("Yaml error in file {}.\n'{}' key is not a dictionary. Value found is {}.",
                            file_name, key, yaml_to_string(dict, 1));
            }
        }

        fn add_prefs(map: &mut PreferenceHashMap, new_prefs: &Yaml, name_prefix: &str, file_name: &str) {
            if new_prefs.is_badvalue() || new_prefs.as_hash().is_none() {
                return;
            }
            let new_prefs = new_prefs.as_hash().unwrap();
            for (yaml_name, yaml_value) in new_prefs {
                let name = as_str_checked(yaml_name);
                if let Err(e) = name {
                    print_errors(&e.chain_err(||
                        format!("name '{}' is not a string in file {}", yaml_to_string(yaml_name, 0), file_name)));                   
                } else if yaml_value.as_hash().is_some() {
                        add_prefs(map, yaml_value, &(name.unwrap().to_string() + "_"), file_name);
                } else if yaml_value.as_vec().is_some() {
                    eprintln!("name '{}' has illegal array value {} in file '{}'",
                            yaml_to_string(yaml_name, 0), yaml_to_string(yaml_value, 0), file_name);
                    return;
                } else {
                    let trimmed_name = name_prefix.to_string() + name.unwrap().trim();
                    let mut trimmed_yaml_value = yaml_value.to_owned();
                    if let Some(value) = trimmed_yaml_value.as_str() {
                        trimmed_yaml_value = Yaml::String(value.trim().to_string());
                    }
                    map.insert(trimmed_name, trimmed_yaml_value);
                }
            }
        }
    }

    /// returns value associated with 'name' or ""
    fn to_string(&self, name: &str) -> String {
        let value = self.prefs.get(name);
        return match value {
            None => "".to_string(),
            Some(v) => match v {
                Yaml::String(s) => s.clone(),
                Yaml::Boolean(b)   => b.to_string(),
                Yaml::Integer(i)    => i.to_string(),
                Yaml::Real(s) => s.clone(),
                _  => "".to_string(),       // shouldn't happen
            }
        }
    }

    #[allow(dead_code)]     // used in testing
    fn set_string_value(&mut self, name: &str, value: &str) {
        self.prefs.insert(name.to_string(), Yaml::String(value.trim().to_string()));
    }

    #[allow(dead_code)]     // used in testing
    fn set_bool_value(&mut self, name: &str, value: bool) {
        self.prefs.insert(name.to_string(), Yaml::Boolean(value));
    }
}


use std::fmt; 
impl fmt::Display for Preferences {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        return write!(f, "{:?}", self);
    }
}

impl fmt::Display for PreferenceManager {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        return write!(f, "{:?}", self);
    }
}


/// When looking for a file, there are up to three possible locations tracked by this type
/// in a non-error situation, at least the first slot should be Some(...).
///
/// Preferences can be found in a few places:
/// 1. Language-independent prefs found in the Rules dir
/// 2. Language-specific prefs
/// 3. Language-region-specific prefs
/// If there are multiple definitions, the later ones overwrite the former ones.
/// This means that region-specific variants will overwrite more general variants.
///
/// Note: the first entry is the first thing found, which might be '2' or '3' in the list above.
pub type Locations = [Option<PathBuf>; 3];

#[derive(Debug, Clone)]
struct FileAndTime {
    files: Locations,
    time: Option<SystemTime>       // ~time file was read (used to see if it was updated and needs to be re-read) 
}

thread_local!{
    static DEFAULT_USER_PREFERENCES: Preferences = Preferences::user_defaults();
    static DEFAULT_API_PREFERENCES: Preferences = Preferences::api_defaults();
}

// FIX: this should be a macro with args the same as println!
fn cannot_go_on(message: &str) {
    // FIX: write this to a log
    eprintln!("{}", message);
    ::std::process::exit(1);
}

/// PreferenceManager keeps track of user and api prefs along with current files
///
/// If one one the `FileAndTime` files changes while the program is running, the values will auto-update
/// Among other things, that means that a UI that changes a user pref will be reflected the next time someone gets speech, braille, etc.
#[derive(Debug)]
pub struct PreferenceManager {
    user_prefs: Preferences,
    api_prefs: Preferences,
    pref_files: FileAndTime,        // the "raw" user preference files (converted to 'user_prefs')
    style: FileAndTime,             // the speech rule style file(s)
    unicode: FileAndTime,           // unicode.yaml file(s)
    defs: FileAndTime,              // the definition.yaml file(s)
}


impl PreferenceManager {
    /// Create (the) PreferenceManager on the heap. 
    pub fn new() -> Box<PreferenceManager> {
        // first, read in the preferences -- need to determine which files to read next
        // the prefs files are in the rules dir and the user dir; differs from other files
        let rules_dir = PreferenceManager::get_rules_dir();
        let (user_prefs, pref_files) = Preferences::from_file(&rules_dir);

        let [style, unicode, defs] =
                PreferenceManager::get_all_files(&rules_dir, &user_prefs);

        return Box::new(
            PreferenceManager {
                user_prefs,
                api_prefs: Preferences{ prefs: DEFAULT_API_PREFERENCES.with(|defaults| defaults.prefs.clone()) },
                pref_files,
                style,
                unicode,
                defs,
            }
        )
    }

    /// Return a `PreferenceHashMap` that is the merger of the api prefs into the user prefs.
    pub fn merge_prefs(&self) -> PreferenceHashMap {
        let mut merged_prefs = self.user_prefs.prefs.clone();
        merged_prefs.extend(self.api_prefs.prefs.clone());
        return merged_prefs;
    }

    fn get_all_files(rules_dir: &PathBuf, prefs: &Preferences) -> [FileAndTime; 3] {
        // try to find ./Rules/lang/style.yaml and ./Rules/lang/style.yaml
        // we go through a series of fallbacks -- we try to maintain the language if possible

        let style_file_name = prefs.to_string("SpeechStyle") + "_Rules.yaml";
        // FIX: should look for other style files in the same language dir if one is not found before move to default
        
        let language = prefs.to_string("Language");
        let language = language.as_str();
        let style_files = PreferenceManager::get_file_and_time(
                            &rules_dir, language, Some("en"), &style_file_name);

        let unicode_files = PreferenceManager::get_file_and_time(
                            &rules_dir, language, Some("en"), "unicode.yaml");

        let defs_files = PreferenceManager::get_file_and_time(
            &rules_dir, language, Some("en"), "definitions.yaml");

        return [style_files, unicode_files, defs_files];
    }


    fn get_file_and_time(rules_dir: &PathBuf, lang: &str, default_lang: Option<&str>, file_name: &str) -> FileAndTime {
        let files = PreferenceManager::get_files(rules_dir, lang, default_lang, file_name);
        return FileAndTime {
            time: Some( SystemTime::now() ),
            files
        }
    }

   fn get_files(rules_dir: &PathBuf, lang: &str, default_lang: Option<&str>, file_name: &str) -> Locations {
        // rules_dir: is the root of the search
        //   to that we add the language dir(s)
        //   if file_name doesn't exist in the language dir(s), we try to find it in the default dir
        // returns all the locations of the file_name from Rules downward

        // start by trying to find a dir that exists
        let mut lang_dir = PreferenceManager::get_language_dir(&rules_dir, lang);
        let mut default_lang = default_lang;
        if lang_dir.is_none() {
            // try again with the default lang if there is one
            if !default_lang.is_some() {
                lang_dir = PreferenceManager::get_language_dir(&rules_dir, default_lang.unwrap());
                if lang_dir.is_none() {
                    // We are done for -- MathCAT can't do anything without the required files!
                    cannot_go_on(
                        &format!("Wasn't able to find/read MathCAT required directory: {}\n Initially looked for language specific directory: {}",
                                        rules_dir.to_str().unwrap(), lang)
                    );
                }

                // the default lang dir exists -- prevent retrying with it.
                default_lang = None;
                // FIX: warn that default is being used                            
            }
        }

        // now find the file name in the dirs
        // we start with the deepest dir and walk back to towards Rules
        // since the order of the rules should be Rules, Rules/lang, Rules/lang/region,
        //   found files are added starting at the end
        let mut result: Locations = [None, None, None];
        let mut i = 3;
        for os_path in lang_dir.unwrap().ancestors() {
            let path = PathBuf::from(os_path).join(file_name);
            if path.is_file() {
                i -= 1;
                result[i] =  Some(path);
            };
            if os_path.ends_with("Rules") {
                break;
            }
        }

        if i < 3 {
            result.rotate_left(i);      // move the 'None(s) to the end
            return result;     // found at least one file
        }

        if default_lang.is_some() {
            // didn't find a file -- retry with default
            // FIX: give a warning that default dir is being used
            return PreferenceManager::get_files(rules_dir, default_lang.unwrap(), None, file_name);
        }
        
        // We are done for -- MathCAT can't do anything without the required files!
        cannot_go_on(
            &format!("Wasn't able to find/read MathCAT required directory: {}\n Initially looked for language specific directory: {}",
                            rules_dir.to_str().unwrap(), lang)
        );

        // will never get here
        return result;
    }

    fn get_language_dir(rules_dir: &PathBuf, lang: &str) -> Option<PathBuf> {
        // return 'Rules/fr', 'Rules/en/gb', etc, if they exist.
        // fall back to main language, and then to default_dir if language dir doesn't exist
        let mut full_path = rules_dir.clone();
        let lang_parts = lang.split("-");
        for part in lang_parts {
            full_path.push(Path::new(part));
            if !full_path.is_dir() {
                break;
            }
        }

        // make sure something got added...
        if rules_dir == &full_path {
            return None;    // didn't find a dir
        } else {
            return Some(full_path);
        }
    }
    
    fn get_rules_dir() -> PathBuf {
        let rules_dir;
        if let Ok(dir) = env::var("MathCATRulesDir") {
           rules_dir = PathBuf::from(dir);
        } else {
            rules_dir = env::current_exe().unwrap().join("Rules");
        }
        if !rules_dir.is_dir() {
            // FIX: handle errors
            cannot_go_on(&format!("Could not find rules dir in {} or lacking permissions to read the dir!",
                                        rules_dir.display()));
        };
        return rules_dir;
    }

    pub fn is_up_to_date(&self) -> bool {
        // FIX: handle errs
        return PreferenceManager::is_file_up_to_date(&self.pref_files) &&
               PreferenceManager::is_file_up_to_date(&self.style) &&
               PreferenceManager::is_file_up_to_date(&self.unicode) &&
               PreferenceManager::is_file_up_to_date(&self.defs) ;
    }

    fn is_file_up_to_date(ft: &FileAndTime) -> bool {
        if ft.time.is_none() {
            // wasn't able to determine a time -- just claim it is up to date
            return true;
        }
        let time = ft.time.unwrap();
        return  is_older(&ft.files[0], time) &&
                is_older(&ft.files[1], time) &&
                is_older(&ft.files[2], time);

        fn is_older(path: &Option<PathBuf>, time: SystemTime) -> bool {
            // if let Some(path_buf) = path {
            //     println!("p={}", path_buf.to_str().unwrap());
            //     println!("p.metadata={:?}", path_buf.metadata());
            //     println!("p.metadata.modified={:?}", path_buf.metadata().unwrap().modified());
            //     println!("p.metadata.modified.duration_since={:?}", path_buf.metadata().unwrap().modified().unwrap().duration_since(time));    
            // }
            return match path {
                Some(p) => {
                    let file_mod_time = p.metadata().unwrap().modified().unwrap();
                    return file_mod_time <= time;
                },
                None => true,
            }           
        }
    }

    /// Return the speech rule style file locations.
    pub fn get_style_file(&self) -> &Locations {
        return &self.style.files;
    }

    /// Return the unicode.yaml file locations.
    pub fn get_unicode_file(&self) -> &Locations {
        return &self.unicode.files;
    }

    /// Return the definitions.yaml file locations.
    pub fn get_definitions_file(&self) -> &Locations {
        return &self.defs.files;
    }

    /// Return the TTS engine currently in use.
    pub fn get_tts(&self) -> TTS {
        return match self.api_prefs.to_string("TTS").as_str() {
            "none" => TTS::None,
            "ssml" => TTS::SSML,
            "sapi5" => TTS::SAPI5,
            _ => {
                eprintln!("found unknown value for TTS: '{}'", self.api_prefs.to_string("TTS").as_str());
                TTS::None
            }
        }
    }

    /// Set the string-valued preference.
    pub fn set_api_string_pref(&mut self, key: String, value: String) {
        self.api_prefs.prefs.insert(key, Yaml::String(value));
    }

    /// Set the number-valued preference.
    /// All number-valued preferences are stored with type `f64`.
    pub fn set_api_float_pref(&mut self, key: String, value: f64) {
        self.api_prefs.prefs.insert(key, Yaml::Real(value.to_string()));
    }

    /// Return the current speech rate.
    pub fn get_rate(&self) -> f64 {
        return match &self.api_prefs.to_string("Rate").parse::<f64>() {
            Ok(val) => *val,
            Err(_) => {
                eprintln!("Rate ('{}') can't be converted to a floating point number", &self.api_prefs.to_string("Rate"));
                DEFAULT_API_PREFERENCES.with(|defaults| defaults.prefs["Rate"].as_f64().unwrap())
            }
        };
    }

    /// Return the current language. The will be the most specific version (e.g, "en-gb")
    pub fn get_language(&self) -> String {
        return self.user_prefs.to_string("Language");
    }

    #[allow(dead_code)]     // used in testing
    fn get_api_prefs(&self) -> &Preferences {
        return &self.api_prefs;
    }

    #[allow(dead_code)]     // used in testing
    fn get_user_prefs(&self) -> &Preferences {
        return &self.user_prefs;
    }

    // occasionally useful to check a pref value when debugging
    // fn get_pref(&self, pref_name: &str) -> String {
    //     return yaml_to_string(self.user_prefs.prefs.get(pref_name).unwrap(), 1);
    // }

    #[allow(dead_code)]
    /// Used in testing, sets the user preference `name` to `value`
    pub fn set_user_prefs(&mut self, name: &str, value: &str) {
        self.user_prefs.set_string_value(name, value);
        if name == "Language" || name == "SpeechStyle" {
            let rules_dir = PreferenceManager::get_rules_dir();   
            let [style, unicode, defs] =
                    PreferenceManager::get_all_files(&rules_dir, &self.user_prefs);
            self.style = style;
            self.unicode = unicode;
            self.defs = defs;
        }
    }
}


#[cfg(test)]
mod tests {

    // For these tests, it is assumed that there are Rules subdirs zz and zz/aa dir; there is no zz/ab
    // definitions.yaml is in Rules, zz, aa dirs
    // unicode.yaml is in zz
    // ClearSpeak_Rules.yaml is in zz
    use super::*;

    /**
     * Return a relative path to Rules dir (ie, .../Rules/zz... returns zz/...)
     */

    // strip .../Rules from file path
    fn rel_path(path: &Option<PathBuf>) -> Option<PathBuf> {
        if let Some(path) = path {
            let path_to_rules_dir = PreferenceManager::get_rules_dir();
            let stripped_path = path.strip_prefix(path_to_rules_dir).unwrap();
            return Some(stripped_path.to_path_buf());
        } else {
            return None;
        }
    }

    fn default_prefs() -> Preferences {
        return DEFAULT_USER_PREFERENCES.with(|defaults| defaults.clone());
    }

    #[test]
    fn find_simple_style() {
        
        let [style, _, _] = PreferenceManager::get_all_files(
                        &PreferenceManager::get_rules_dir(), &default_prefs());

        assert_eq!(rel_path(&style.files[0]), Some(PathBuf::from("en/ClearSpeak_Rules.yaml")));
    }

    #[test]
    fn find_style_other_language() {
        let mut prefs = default_prefs();
        prefs.set_string_value("Language", "zz");
        let [style, _, _] = PreferenceManager::get_all_files(
            &PreferenceManager::get_rules_dir(), &prefs);

        assert_eq!(rel_path(&style.files[0]), Some(PathBuf::from("zz/ClearSpeak_Rules.yaml")));
    }

    #[test]
    fn find_unicode_files() {
        let mut prefs = default_prefs();
        prefs.set_string_value("Language", "zz-aa");
        let [style, _, _] = PreferenceManager::get_all_files(
            &PreferenceManager::get_rules_dir(), &prefs);

        assert_eq!(rel_path(&style.files[0]), Some(PathBuf::from("zz/ClearSpeak_Rules.yaml")));
        assert_eq!(rel_path(&style.files[1]), Some(PathBuf::from("zz/aa/ClearSpeak_Rules.yaml")));
    }

    #[test]
    fn find_style_no_sublanguage() {
        let mut prefs = default_prefs();
        prefs.set_string_value("Language", "zz-ab");
        let [style, _, _] = PreferenceManager::get_all_files(
            &PreferenceManager::get_rules_dir(), &prefs);

        assert_eq!(rel_path(&style.files[0]), Some(PathBuf::from("zz/ClearSpeak_Rules.yaml")));
    }

    #[test]
    fn found_all_files() {
        fn count_files(ft: &FileAndTime) -> usize {
            return ft.files.iter().filter(|path| path.is_some()).count();
        }
        fn assert_helper(count: usize, correct: usize, file_name: &str) {
            assert_eq!(count, correct, "In looking for '{}', found {} files, should have found {}", 
                    file_name, count, correct);
        }

        let mut prefs = default_prefs();
        prefs.set_string_value("Language", "zz-aa");
        let [style, unicode, defs] = PreferenceManager::get_all_files(
            &PreferenceManager::get_rules_dir(), &prefs);

        assert_helper(count_files(&style), 2, "ClearSpeak_Rules.yaml");
        assert_helper(count_files(&unicode), 1, "unicode.yaml");
        assert_helper(count_files(&defs), 3,"definitions.yaml");

        prefs.set_string_value("Language", "zz-ab");
        let [style, unicode, defs] = PreferenceManager::get_all_files(
            &PreferenceManager::get_rules_dir(), &prefs);

        assert_helper(count_files(&style), 1, "ClearSpeak_Rules.yaml");
        assert_helper(count_files(&unicode), 1, "unicode.yaml");
        assert_helper(count_files(&defs), 2, "definitions.yaml");
    }

    #[test]
    fn file_found_order() {
        let mut prefs = default_prefs();
        prefs.set_string_value("Language", "zz-aa");
        let [_, _, defs] = PreferenceManager::get_all_files(
            &PreferenceManager::get_rules_dir(), &prefs);

        let mut iter = defs.files.iter();
        assert_eq!(rel_path(iter.next().unwrap()).unwrap(), Path::new("definitions.yaml"));
        assert_eq!(rel_path(iter.next().unwrap()).unwrap(), Path::new("zz/definitions.yaml"));
        assert_eq!(rel_path(iter.next().unwrap()).unwrap(), Path::new("zz/aa/definitions.yaml"));
        assert_eq!(iter.next(), None, "Should not be any files left")
    }

    #[test]
    fn test_prefs() {
        let pref_manager = PreferenceManager::new();
        let prefs = pref_manager.get_user_prefs();
        assert_eq!(prefs.to_string("Language").as_str(), "en");
        assert_eq!(prefs.to_string("SubjectArea").as_str(), "general");
        assert_eq!(prefs.to_string("ClearSpeak_AbsoluteValue").as_str(), "Auto");
        assert_eq!(prefs.to_string("ResetStartMode").as_str(), "false");
        assert_eq!(prefs.to_string("Code").as_str(), "Nemeth");
        assert_eq!(prefs.to_string("X_Y_Z").as_str(), "");
    }

    #[test]
    fn test_language_change() {
        let mut pref_manager = PreferenceManager::new();
        assert_eq!(rel_path(&pref_manager.get_style_file()[0]), Some(PathBuf::from("en/ClearSpeak_Rules.yaml")));

        pref_manager.set_user_prefs("Language", "zz");
        assert_eq!(rel_path(&pref_manager.get_style_file()[0]), Some(PathBuf::from("zz/ClearSpeak_Rules.yaml")));
    }

    #[test]
    fn test_is_up_to_date() {
        let mut pref_manager = PreferenceManager::new();
        pref_manager.set_user_prefs("Language", "zz-ab");        
        assert!(pref_manager.is_up_to_date());
    }

    use std::fs;
    #[test]
    fn test_is_not_up_to_date() {
        use std::thread::sleep;
        use std::time::Duration;
        let mut pref_manager = PreferenceManager::new();
        pref_manager.set_user_prefs("Language", "zz-aa");        
        sleep(Duration::from_millis(10));

        // Note: need to use pattern match to avoid borrow problem
        if let Some(file_name) = &pref_manager.get_style_file()[0] {
            let file_name_as_str = file_name.to_str().unwrap();
            let contents = fs::read(file_name).expect(&format!("Failed to write file {} during test", file_name_as_str));
            #[allow(unused_must_use)] { 
                fs::write(file_name, contents);
            }
            assert!(!pref_manager.is_up_to_date());    
        } else {
            panic!("First path is 'None'");
        }

        // open the file, read all the contents, then write them back so the time changes

    }
}
