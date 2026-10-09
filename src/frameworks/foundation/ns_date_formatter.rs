/*
 * This Source Code Form is subject to the terms of the Mozilla Public
 * License, v. 2.0. If a copy of the MPL was not distributed with this
 * file, You can obtain one at https://mozilla.org/MPL/2.0/.
 */
//! `NSDateFormatter`.
//!
//! Resources:
//! - Apple's [Introduction to Data Formatting Programming Guide For Cocoa](https://developer.apple.com/library/archive/documentation/Cocoa/Conceptual/DataFormatting/DataFormatting.html)
//! - [Unicode Technical Standard #35](https://unicode.org/reports/tr35/tr35-10.html#Date_Format_Patterns)

use crate::frameworks::core_foundation::time::CFAbsoluteTimeGetGregorianDate;
use crate::frameworks::foundation::{ns_string, NSTimeInterval};
use crate::objc::{
    autorelease, id, msg, nil, objc_classes, todo_objc_setter, ClassExports, HostObject, NSZonePtr,
};

struct NSDateFormatterHostObject {
    date_format: Option<id>,
}
impl HostObject for NSDateFormatterHostObject {}

pub const CLASSES: ClassExports = objc_classes! {

(env, this, _cmd);

@implementation NSDateFormatter: NSObject

+ (id)allocWithZone:(NSZonePtr)_zone {
    let host_object = Box::new(NSDateFormatterHostObject {
        date_format: None,
    });
    env.objc.alloc_object(this, host_object, &mut env.mem)
}

- (())setDateFormat:(id)format { // NSString *
    let date_format: id = msg![env; format copy];
    env.objc.borrow_mut::<NSDateFormatterHostObject>(this).date_format = Some(date_format);
}

- (())setTimeZone:(id)time_zone {
    todo_objc_setter!(this, time_zone);
}

- (id)stringFromDate:(id)date {
    let &NSDateFormatterHostObject { date_format } = env.objc.borrow(this);
    let format = ns_string::to_rust_string(env, date_format.unwrap()).to_string().clone();
    log_dbg!("date_format before: {:?}", format);

    let ti: NSTimeInterval = msg![env; date timeIntervalSinceReferenceDate];
    let greg_date = CFAbsoluteTimeGetGregorianDate(env, ti, nil);
    let year = greg_date.year;
    let month = greg_date.month;
    let day = greg_date.day;
    let hour = greg_date.hours;
    let minute = greg_date.minutes;
    let second = greg_date.seconds;

    // Day of week: Jan 1 2001 (absolute time epoch) was a Monday.
    let days_since_epoch = (ti / 86400.0).floor() as i64;
    let weekday_idx = ((days_since_epoch + 1) % 7 + 7) % 7;
    let weekday_names = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"];
    let full_weekday_names = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];
    let month_names = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

    let hour12 = if hour % 12 == 0 { 12 } else { hour % 12 };
    let am_pm = if hour < 12 { "AM" } else { "PM" };

    let mut res = String::new();
    let chars: Vec<char> = format.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        let c = chars[i];
        if c == '\'' {
            // Quoted literal text; '' is an escaped single quote.
            i += 1;
            let mut saw_quote = false;
            while i < chars.len() {
                if chars[i] == '\'' {
                    if saw_quote {
                        res.push('\'');
                        saw_quote = false;
                    } else if i + 1 < chars.len() && chars[i + 1] == '\'' {
                        saw_quote = true;
                    } else {
                        break;
                    }
                    i += 1;
                } else {
                    res.push(chars[i]);
                    i += 1;
                }
            }
            i += 1; // skip closing quote
        } else if c.is_ascii_alphabetic() {
            let mut j = i;
            while j < chars.len() && chars[j] == c {
                j += 1;
            }
            let run = j - i;
            let padded = |v: i64, len: usize| -> String {
                let s = format!("{v}");
                if s.len() >= len { s } else { format!("{}{}", "0".repeat(len - s.len()), s) }
            };
            match c {
                'y' | 'Y' | 'u' => res.push_str(&padded(year as i64, run.min(4))),
                'M' | 'L' => {
                    if run >= 3 {
                        res.push_str(month_names[(month as usize).clamp(1, 12) - 1]);
                    } else {
                        res.push_str(&padded(month as i64, run));
                    }
                }
                'd' => res.push_str(&padded(day as i64, run)),
                'D' => res.push_str(&padded(1, run)),
                'E' => {
                    if run >= 4 {
                        res.push_str(full_weekday_names[weekday_idx as usize]);
                    } else {
                        res.push_str(weekday_names[weekday_idx as usize]);
                    }
                }
                'H' | 'k' => {
                    let v = if c == 'k' && hour == 0 { 24 } else { hour };
                    res.push_str(&padded(v as i64, run));
                }
                'K' | 'h' => {
                    let v = if c == 'K' { hour % 12 } else { hour12 };
                    res.push_str(&padded(v as i64, run));
                }
                'm' => res.push_str(&padded(minute as i64, run)),
                's' => res.push_str(&padded(second as i64, run)),
                'S' => {
                    let frac = second - second.floor();
                    let millis = (frac * 1000.0).round() as i64;
                    res.push_str(&padded(millis, run.min(3)));
                }
                'a' => res.push_str(am_pm),
                'Z' | 'z' | 'v' | 'V' | 'X' | 'x' => res.push_str(if c == 'Z' && run >= 4 { "+0000" } else { "Z" }),
                'G' => res.push_str("AD"),
                'w' | 'W' | 'F' | 'e' | 'c' => res.push_str(&padded(1, 1)),
                _ => unimplemented!("unimplemented date format pattern: {c}"),
            }
            i = j;
        } else {
            res.push(c);
            i += 1;
        }
    }

    log_dbg!("date_format after: {:?}", res);

    let out = ns_string::from_rust_string(env, res);
    autorelease(env, out)
}

@end

};
