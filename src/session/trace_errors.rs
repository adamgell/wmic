//! TRACE descriptions from the original English WMIC resource lookup.
//! The observed native switch is 0x140012fb0 in ARM64 WMIC, SHA256
//! 2ef794c4940afea56c6d6d3b9bc9e66965bc7333e79c5dcb2def3c0e36f0834c.
//! Resource source: en-US wmic.exe.mui, SHA256
//! 5e3bc78af6eb1aea40c6899c452ca8701d28756bf1eb5f056207fc963cfea17b.
//! Return None for unmapped codes and the native 0x8004105a lookup of an absent resource.
//! One terminating resource CRLF is omitted; internal paragraph breaks are preserved.
pub(in crate::session) fn description(code: u32) -> Option<&'static str> {
    Some(match code {
        0x00000000 => "The operation was successful.",
        0x00000001 => {
            "This will be returned when no more objects are available, the number of objects returned is less than the number requested, or at the end of an enumeration. It is also returned when this method is called with a value of zero for the uCount parameter. "
        }
        0x00040001 => "An attempt was made to create an object or class that already exists.",
        0x00040002 => {
            "An overridden property was deleted.\r\n\r\nThis value is returned to signal that the original non-overridden value has been restored as a result of the deletion."
        }
        0x00040003 => "The items (objects, classes, and so on) being compared are not identical.",
        0x00040004 => {
            "A call timed out.\r\n\r\nThis is not an error condition. Therefore, some results may have also been returned.\r\n "
        }
        0x00040005 => {
            "No more data is available from the enumeration, and the user should terminate the enumeration."
        }
        0x00040006 => "The operation was intentionally or unintentionally canceled. ",
        0x00040007 => "A request is still in progress, and the results are not yet available.",
        0x00040008 => {
            "More that one copy of the same object was detected in the result set of an enumeration."
        }
        0x00040009 => "The user was denied access to some but not all resources.",
        0x00040010 => {
            "The user did not receive all of the objects requested due to inaccessible resources (other than security violations)."
        }
        0x80041001 => "The call failed.",
        0x80041002 => "The object could not be found.",
        0x80041003 => "The current user does not have permission to perform the action",
        0x80041004 => "The provider has failed at some time other than during initialization.",
        0x80041005 => "A type mismatch occurred.",
        0x80041006 => "There was not enough memory for the operation.",
        0x80041007 => "The IWbemContext object is not valid.",
        0x80041008 => "One of the parameters to the call is not correct.",
        0x80041009 => "The resource, typically a remote server, is not currently available.",
        0x8004100a => {
            "An internal, critical, and unexpected error occurred. Report this error to Microsoft Technical Support."
        }
        0x8004100b => "One or more network packets were corrupted during a remote session.",
        0x8004100c => "The feature or operation is not supported.",
        0x8004100d => "The superclass specified is not valid.",
        0x8004100e => "The namespace specified could not be found.",
        0x8004100f => "The specified instance is not valid.",
        0x80041010 => "The specified class is not valid.",
        0x80041011 => {
            "A provider referenced in the schema does not have a corresponding registration."
        }
        0x80041012 => {
            "A provider referenced in the schema has an incorrect or incomplete registration. This error may be caused by any of the following\r\n \r\nA missing pragma namespace command in the MOF file used to register the provider, resulting in the provider being registered in the wrong WMI namespace. \r\n\r\nA corrupt repository, which may be fixed by deleting it and recompiling the MOF file. \r\n"
        }
        0x80041013 => {
            "COM cannot locate a provider referenced in the schema. This error may be caused by any of the following:\r\n\r\nThe provider is using a WMI DLL that does not match the .lib file used when the provider was built. \r\n\r\nThe provider's DLL or any of the DLLs on which it depends is corrupt.\r\n \r\nThe provider failed to export DllRegisterServer. \r\n\r\nAn in-process provider was not registered using the regsvr32 command. \r\n\r\nAn out-of-process provider that was not registered using the /regserver switch. For example, myprog.exe /regserver.\r\n"
        }
        0x80041014 => "A component, such as a provider, failed to initialize for internal reasons.",
        0x80041015 => "A networking error that prevents normal operation has occurred.",
        0x80041016 => {
            "The requested operation is not valid. This error usually applies to invalid attempts to delete classes or properties."
        }
        0x80041017 => "The query was not syntactically valid.",
        0x80041018 => "The requested query language is not supported.",
        0x80041019 => {
            "In a put operation, the wbemChangeFlagCreateOnly flag was specified, but the instance already exists"
        }
        0x8004101a => {
            "It is not possible to perform the add operation on this qualifier because the owning object does not permit overrides."
        }
        0x8004101b => {
            "The user attempted to delete a qualifier that was not owned. The qualifier was inherited from a parent class."
        }
        0x8004101c => {
            "The user attempted to delete a property that was not owned. The property was inherited from a parent class."
        }
        0x8004101d => {
            "The client made an unexpected and illegal sequence of calls, such as calling EndEnumeration before calling BeginEnumeration."
        }
        0x8004101e => {
            "The user requested an illegal operation, such as spawning a class from an instance."
        }
        0x8004101f => {
            "There was an illegal attempt to specify a key qualifier on a property that cannot be a key. The keys are specified in the class definition for an object, and cannot be altered on a per-instance basis. "
        }
        0x80041020 => {
            "The current object is not a valid class definition. Either it is incomplete, or it has not been registered with WMI using SWbemObject.Put_."
        }
        0x80041021 => "Reserved for future use.",
        0x80041022 => "Reserved for future use.",
        0x80041023 => "The property that you are attempting to modify is read-only. ",
        0x80041024 => {
            "The provider cannot perform the requested operation. This would include a query that is too complex, retrieving an instance, creating or updating a class, deleting a class, or enumerating a class."
        }
        0x80041025 => "An attempt was made to make a change that would invalidate a subclass.\r\n",
        0x80041026 => "An attempt has been made to delete or modify a class that has instances.",
        0x80041027 => "Reserved for future use.",
        0x80041028 => {
            "A value of Nothing was specified for a property that may not be Nothing, such as one that is marked by a Key, Indexed, or Not_Null qualifier."
        }
        0x80041029 => {
            "The variant value for a qualifier was provided that is not of a legal qualifier type."
        }
        0x8004102a => "The CIM type specified for a property is not valid.",
        0x8004102b => {
            "The request was made with an out-of-range value, or is incompatible with the type."
        }
        0x8004102c => {
            "An illegal attempt was made to make a class singleton, such as when the class is derived from a non-singleton class"
        }
        0x8004102d => "The CIM type specified is not valid. ",
        0x8004102e => "The requested method is not available.",
        0x8004102f => "The parameters provided for the method are not valid.",
        0x80041030 => "There was an attempt to get qualifiers on a system property.",
        0x80041031 => "The CIM type specified for a property is not valid.",
        0x80041032 => {
            "An asynchronous process has been canceled internally or by the user. Note that due to the timing and nature of the asynchronous operation the operation may not have been truly canceled."
        }
        0x80041033 => {
            "The user has requested an operation while WMI is in the process of shutting down."
        }
        0x80041034 => {
            "An attempt was made to reuse an existing method name from a superclass, and the signatures did not match."
        }
        0x80041035 => {
            "One or more parameter values, such as a query text, is too complex or unsupported. WMI is therefore requested to retry the operation with simpler parameters."
        }
        0x80041036 => "A parameter was missing from the method call.",
        0x80041037 => "A method parameter has an invalid ID qualifier.",
        0x80041038 => {
            "One or more of the method parameters have ID qualifiers that are out of sequence."
        }
        0x80041039 => "The return value for a method has an ID qualifier.",
        0x8004103a => "The specified object path was invalid.",
        0x8004103b => "There is not enough free disk space to continue the operation. ",
        0x8004103c => {
            "The supplied buffer was too small to hold all the objects in the enumerator or to read a string property."
        }
        0x8004103d => "The provider does not support the requested put operation.",
        0x8004103e => {
            "An object with an incorrect type or version was encountered during marshaling."
        }
        0x8004103f => {
            "A packet with an incorrect type or version was encountered during marshaling."
        }
        0x80041040 => "The packet has an unsupported version.",
        0x80041041 => "The packet appears to be corrupted.",
        0x80041042 => {
            "The variant value for a qualifier was provided that is not of a legal qualifier type. "
        }
        0x80041043 => "A duplicate parameter has been declared in a CIM method.",
        0x80041044 => "A duplicate parameter has been declared in a CIM method.",
        0x80041045 => {
            "A call to IWbemObjectSink::Indicate has failed. The provider may choose to refire the event."
        }
        0x80041046 => "The specified flavor was invalid.",
        0x80041047 => {
            "An attempt has been made to create a reference that is circular (for example, deriving a class from itself)."
        }
        0x80041048 => "The specified class is not supported.",
        0x80041049 => {
            "An attempt was made to change a key when instances or subclasses are already using the key."
        }
        0x80041050 => {
            "An attempt was made to change an index when instances or subclasses are already using the index."
        }
        0x80041051 => {
            "An attempt was made to create more properties than the current version of the class supports."
        }
        0x80041052 => "A property was redefined with a conflicting type in a derived class.",
        0x80041053 => {
            "An attempt was made in a derived class to override a non-overrideable qualifier."
        }
        0x80041054 => "A method was redeclared with a conflicting signature in a derived class.",
        0x80041055 => {
            "An attempt was made to execute a method not marked with [implemented] in any relevant class."
        }
        0x80041056 => "An attempt was made to execute a method marked with [disabled].",
        0x80041057 => "The refresher is busy with another operation.",
        0x80041058 => "The filtering query is syntactically invalid.",
        0x80041059 => {
            "The FROM clause of a filtering query references a class that is not an event class (not derived from __Event)."
        }
        0x8004105b => "A GROUP BY clause was used. Aggregation on all properties is not supported.",
        0x8004105c => "Dot notation was used on a property that is not an embedded object.",
        0x8004105d => {
            "A GROUP BY clause references a property that is an embedded object without using dot notation."
        }
        0x8004105f => {
            "An event provider registration query (__EventProviderRegistration) did not specify the classes for which events were provided."
        }
        0x80041060 => {
            "An request was made to back up or restore the repository while WinMgmt.exe was using it. "
        }
        0x80041061 => {
            "The asynchronous delivery queue overflowed due to the event consumer being too slow."
        }
        0x80041062 => {
            "The operation failed because the client did not have the necessary security privilege."
        }
        0x80041063 => "The operator is not valid for this property type.",
        0x80041064 => {
            "The user specified a username/password/authority on a local connection. The user must use a blank username/password and rely on default security."
        }
        0x80041065 => "The class was made abstract when its superclass is not abstract. ",
        0x80041066 => {
            "An amended object was PUT without the WBEM_FLAG_USE_AMENDED_QUALIFIERS flag being specified."
        }
        0x80041067 => "The client was not retrieving objects quickly enough from an enumeration.",
        0x80042001 => "The provider registration overlaps with the system event domain.",
        0x80042002 => "A WITHIN clause was not used in this query.",
        _ => return None,
    })
}

#[cfg(test)]
mod tests {
    use super::description;

    #[test]
    fn traced_wmi_failures_use_the_original_verbose_resource_text() {
        for (code, expected) in [
            (0x80041002, "The object could not be found."),
            (0x8004100e, "The namespace specified could not be found."),
            (0x80041017, "The query was not syntactically valid."),
            (0x80041010, "The specified class is not valid."),
        ] {
            assert_eq!(description(code), Some(expected), "{code:08x}");
        }
    }

    #[test]
    fn absent_and_invalid_resource_cases_allow_the_existing_error_fallback() {
        for code in [
            0x800706ba, 0x80070005, 0x8004104a, 0x8004105a, 0x8004105e, 0x80041068,
        ] {
            assert_eq!(description(code), None, "{code:08x}");
        }
    }

    #[test]
    fn later_switch_ranges_use_their_explicit_resource_mappings() {
        for (code, expected) in [
            (
                0x80041030,
                "There was an attempt to get qualifiers on a system property.",
            ),
            (
                0x80041050,
                "An attempt was made to change an index when instances or subclasses are already using the index.",
            ),
            (0x80042002, "A WITHIN clause was not used in this query."),
        ] {
            assert_eq!(description(code), Some(expected), "{code:08x}");
        }
    }

    #[test]
    fn resource_paragraphs_are_preserved_without_the_final_crlf() {
        assert_eq!(
            description(0x00040002),
            Some(concat!(
                "An overridden property was deleted.\r\n\r\n",
                "This value is returned to signal that the original non-overridden value has been restored as a result of the deletion."
            ))
        );
    }
}
