// SPDX-License-Identifier: MIT
pragma solidity >=0.5.12;

contract NeonToken {
    address constant NeonPrecompiled = 0xFF00000000000000000000000000000000000003;

    function withdraw(bytes32 spender) public payable returns (bool) {
        (bool success, bytes memory returnData) = NeonPrecompiled.delegatecall(abi.encodeWithSignature("withdraw(bytes32)", spender));
        assert(success, string(returnData));
        return success;
    }
}